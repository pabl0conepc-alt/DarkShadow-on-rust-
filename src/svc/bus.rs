//! D-Bus, on the event loop: MPRIS (music), the notification server, NetworkManager, BlueZ, UPower and
//! logind (brightness). libdbus queues what arrives; we pop it whenever its socket is readable.

use std::collections::HashMap;
use std::os::fd::{AsFd, BorrowedFd, RawFd};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbus::arg::{PropMap, RefArg, Variant};
use dbus::blocking::LocalConnection;
use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
use dbus::channel::{BusType, Channel};
use dbus::message::MessageType;
use dbus::{Message, Path};
use smithay_client_toolkit::reexports::calloop::{
    Interest, Mode, PostAction,
    generic::Generic,
    timer::{TimeoutAction, Timer},
};

use crate::app::App;
use crate::state::{NetKind, Notif, topic};

const T: Duration = Duration::from_millis(800);
const MPRIS: &str = "org.mpris.MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const NOTIF_PATH: &str = "/org/freedesktop/Notifications";
const NOTIF_IFACE: &str = "org.freedesktop.Notifications";

struct Fd(RawFd);
impl AsFd for Fd {
    fn as_fd(&self) -> BorrowedFd<'_> {
        // SAFETY: the descriptor belongs to a connection that lives as long as the shell.
        unsafe { BorrowedFd::borrow_raw(self.0) }
    }
}

#[derive(Default, Clone)]
#[allow(dead_code)]
struct Player {
    unique: String,
    status: String,
}

#[derive(Default)]
pub struct Bus {
    pub session: Option<Rc<LocalConnection>>,
    pub system: Option<Rc<LocalConnection>>,
    players: HashMap<String, Player>,
    /// unique name -> well-known player name
    owners: HashMap<String, String>,
    active: String,
    nm_ap: String,
    bt_armed: bool,
    net_armed: bool,
    art_for: String,
}

fn open(kind: BusType) -> Option<Rc<LocalConnection>> {
    let mut ch = Channel::get_private(kind).ok()?;
    ch.set_watch_enabled(true);
    Some(Rc::new(LocalConnection::from(ch)))
}

pub fn start(app: &mut App) {
    if let Some(c) = open(BusType::Session) {
        let _ = c.add_match_no_cb(
            "type='signal',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged',path='/org/mpris/MediaPlayer2'",
        );
        let _ = c.add_match_no_cb("type='signal',interface='org.mpris.MediaPlayer2.Player',member='Seeked'");
        let _ = c.add_match_no_cb(
            "type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged',arg0namespace='org.mpris.MediaPlayer2'",
        );
        watch(app, &c, false);
        app.svc.bus.session = Some(c);
        if app.cfg.notifications.enabled {
            own_notifications(app);
        }
        scan_players(app);
    }
    if let Some(c) = open(BusType::System) {
        let _ = c.add_match_no_cb(
            "type='signal',sender='org.freedesktop.NetworkManager',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged'",
        );
        let _ = c.add_match_no_cb("type='signal',sender='org.bluez'");
        let _ = c.add_match_no_cb(
            "type='signal',sender='org.freedesktop.UPower',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged'",
        );
        watch(app, &c, true);
        app.svc.bus.system = Some(c);
        refresh_net(app);
        refresh_bt(app);
        refresh_power(app);
        crate::actions::brightness_read(app);
    }
}

fn watch(app: &mut App, c: &Rc<LocalConnection>, system: bool) {
    let fd = c.channel().watch().fd;
    let _ = app.lh.insert_source(Generic::new(Fd(fd), Interest::READ, Mode::Level), move |_, _, app| {
        let c = if system { app.svc.bus.system.clone() } else { app.svc.bus.session.clone() };
        if let Some(c) = c {
            if c.channel().read_write(Some(Duration::ZERO)).is_err() {
                return Ok(PostAction::Remove);
            }
            pump(app, &c, system);
        }
        Ok(PostAction::Continue)
    });
}

fn pump(app: &mut App, c: &Rc<LocalConnection>, system: bool) {
    let mut n = 0;
    while let Some(m) = c.channel().pop_message() {
        if system {
            on_system(app, &m);
        } else {
            on_session(app, c, &m);
        }
        n += 1;
        if n > 500 {
            break;
        }
    }
}

/// Messages that came in while we were waiting for a reply sit in libdbus's queue: take them now.
pub fn drain(app: &mut App) {
    if let Some(c) = app.svc.bus.session.clone() {
        pump(app, &c, false);
    }
    if let Some(c) = app.svc.bus.system.clone() {
        pump(app, &c, true);
    }
}

// ------------------------------------------------------------------ session bus

fn on_session(app: &mut App, c: &Rc<LocalConnection>, m: &Message) {
    match m.msg_type() {
        MessageType::MethodCall => {
            if m.path().as_deref() == Some(NOTIF_PATH) {
                notif_call(app, c, m);
            } else {
                let _ = c.channel().send(unknown(m));
            }
        }
        MessageType::Signal => {
            let member = m.member().map(|s| s.to_string()).unwrap_or_default();
            let sender = m.sender().map(|s| s.to_string()).unwrap_or_default();
            match member.as_str() {
                "NameOwnerChanged" => {
                    let (name, old, new): (String, String, String) = m.read3().unwrap_or_default();
                    if !name.starts_with(MPRIS) {
                        return;
                    }
                    if !old.is_empty() {
                        app.svc.bus.owners.remove(&old);
                        app.svc.bus.players.remove(&name);
                    }
                    if !new.is_empty() {
                        app.svc.bus.owners.insert(new.clone(), name.clone());
                        app.svc.bus.players.insert(name.clone(), Player { unique: new, status: String::new() });
                        read_player(app, &name);
                    }
                    pick_player(app);
                }
                "PropertiesChanged" => {
                    let Some(name) = app.svc.bus.owners.get(&sender).cloned() else { return };
                    let (iface, changed): (String, PropMap) = match m.read2() {
                        Ok(v) => v,
                        Err(_) => return,
                    };
                    if iface != PLAYER {
                        return;
                    }
                    if let Some(s) = changed.get("PlaybackStatus").and_then(|v| v.0.as_str()) {
                        if let Some(p) = app.svc.bus.players.get_mut(&name) {
                            p.status = s.to_string();
                        }
                        if s == "Playing" && app.svc.bus.active != name {
                            app.svc.bus.active = name.clone();
                            read_player(app, &name);
                            apply_active(app);
                            return;
                        }
                    }
                    if name == app.svc.bus.active {
                        apply_props(app, &changed, true);
                    } else {
                        pick_player(app);
                    }
                }
                "Seeked" => {
                    let Some(name) = app.svc.bus.owners.get(&sender).cloned() else { return };
                    if name == app.svc.bus.active {
                        let pos: i64 = m.read1().unwrap_or(0);
                        app.st.media.pos_us = pos;
                        app.st.media.pos_at = Some(Instant::now());
                        app.mark(topic::MEDIA);
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn scan_players(app: &mut App) {
    let Some(c) = app.svc.bus.session.clone() else { return };
    let p = c.with_proxy("org.freedesktop.DBus", "/org/freedesktop/DBus", T);
    let names: (Vec<String>,) = p.method_call("org.freedesktop.DBus", "ListNames", ()).unwrap_or_default();
    for n in names.0.into_iter().filter(|n| n.starts_with(MPRIS)) {
        let owner: (String,) = p.method_call("org.freedesktop.DBus", "GetNameOwner", (n.as_str(),)).unwrap_or_default();
        app.svc.bus.owners.insert(owner.0.clone(), n.clone());
        app.svc.bus.players.insert(n.clone(), Player { unique: owner.0, status: String::new() });
        read_player(app, &n);
    }
    pick_player(app);
}

fn read_player(app: &mut App, name: &str) {
    let Some(c) = app.svc.bus.session.clone() else { return };
    let p = c.with_proxy(name, "/org/mpris/MediaPlayer2", T);
    if let Ok(props) = p.get_all(PLAYER) {
        if let Some(s) = props.get("PlaybackStatus").and_then(|v| v.0.as_str())
            && let Some(pl) = app.svc.bus.players.get_mut(name)
        {
            pl.status = s.to_string();
        }
        if app.svc.bus.active == name {
            apply_props(app, &props, false);
        }
    }
}

/// The player shown: the one playing, else the one shown before, else any.
fn pick_player(app: &mut App) {
    let b = &app.svc.bus;
    let playing = b.players.iter().find(|(_, p)| p.status == "Playing").map(|(n, _)| n.clone());
    let keep = b.players.contains_key(&b.active).then(|| b.active.clone());
    let any = b.players.keys().min().cloned();
    let next = playing.or(keep).or(any).unwrap_or_default();
    if next != app.svc.bus.active || app.st.media.bus != next {
        app.svc.bus.active = next;
        apply_active(app);
    }
}

fn apply_active(app: &mut App) {
    let name = app.svc.bus.active.clone();
    app.st.media = Default::default();
    app.st.media.bus = name.clone();
    if name.is_empty() {
        app.mark(topic::MEDIA);
        crate::shell::media_changed(app);
        return;
    }
    let Some(c) = app.svc.bus.session.clone() else { return };
    let p = c.with_proxy(name.as_str(), "/org/mpris/MediaPlayer2", T);
    if let Ok(id) = p.get::<String>(MPRIS, "Identity") {
        app.st.media.identity = id;
    }
    if let Ok(props) = p.get_all(PLAYER) {
        apply_props(app, &props, false);
    } else {
        app.mark(topic::MEDIA);
    }
}

fn apply_props(app: &mut App, props: &PropMap, partial: bool) {
    let m = &mut app.st.media;
    let was_playing = m.playing;
    if let Some(s) = props.get("PlaybackStatus").and_then(|v| v.0.as_str()) {
        m.playing = s == "Playing";
    }
    if let Some(md) = props.get("Metadata") {
        m.title.clear();
        m.artist.clear();
        m.album.clear();
        m.art_url.clear();
        m.length_us = 0;
        m.track_id.clear();
        if let Some(mut it) = md.0.as_iter() {
            while let (Some(k), Some(v)) = (it.next(), it.next()) {
                let key = k.as_str().unwrap_or("");
                let v = unwrap_variant(v);
                match key {
                    "xesam:title" => m.title = v.as_str().unwrap_or("").to_string(),
                    "xesam:album" => m.album = v.as_str().unwrap_or("").to_string(),
                    "xesam:artist" => {
                        if let Some(mut a) = v.as_iter() {
                            let list: Vec<String> = std::iter::from_fn(|| a.next())
                                .filter_map(|x| x.as_str().map(str::to_string))
                                .collect();
                            m.artist = list.join(", ");
                        } else if let Some(s) = v.as_str() {
                            m.artist = s.to_string();
                        }
                    }
                    "mpris:artUrl" => m.art_url = v.as_str().unwrap_or("").to_string(),
                    "mpris:length" => m.length_us = v.as_i64().or_else(|| v.as_u64().map(|x| x as i64)).unwrap_or(0),
                    "mpris:trackid" => m.track_id = v.as_str().unwrap_or("").to_string(),
                    _ => {}
                }
            }
        }
        if !partial || !props.contains_key("Position") {
            m.pos_us = 0;
            m.pos_at = Some(Instant::now());
        }
    }
    if let Some(p) = props.get("Position").and_then(|v| v.0.as_i64()) {
        m.pos_us = p;
        m.pos_at = Some(Instant::now());
    } else if props.contains_key("PlaybackStatus") || props.contains_key("Metadata") {
        // the position is not signalled: ask for it when playback starts or the track changes
        let name = app.svc.bus.active.clone();
        if let Some(c) = app.svc.bus.session.clone() {
            let p = c.with_proxy(name.as_str(), "/org/mpris/MediaPlayer2", T);
            if let Ok(pos) = p.get::<i64>(PLAYER, "Position") {
                app.st.media.pos_us = pos;
                app.st.media.pos_at = Some(Instant::now());
            }
        }
    }
    let m = &mut app.st.media;
    if !m.playing && was_playing {
        // freeze the position where it stopped
        let now = Instant::now();
        if let Some(at) = m.pos_at {
            m.pos_us += now.saturating_duration_since(at).as_micros() as i64;
        }
        m.pos_at = Some(now);
    }
    if let Some(cs) = props.get("CanSeek").and_then(|v| v.0.as_u64()) {
        m.can_seek = cs != 0;
    }
    let url = m.art_url.clone();
    if url != app.svc.bus.art_for {
        app.svc.bus.art_for = url.clone();
        app.st.media.art = None;
        if !url.is_empty() {
            crate::svc::images::load_art(app, url);
        }
    }
    app.mark(topic::MEDIA);
    crate::shell::media_changed(app);
}

fn unwrap_variant<'a>(v: &'a (dyn RefArg + 'a)) -> &'a (dyn RefArg + 'a) {
    if let Some(mut it) = v.as_iter()
        && v.signature().starts_with('v')
        && let Some(inner) = it.next()
    {
        return inner;
    }
    v
}

pub fn player_call(app: &mut App, method: &str) {
    let name = app.svc.bus.active.clone();
    if name.is_empty() {
        return;
    }
    if let Some(c) = app.svc.bus.session.clone() {
        let p = c.with_proxy(name.as_str(), "/org/mpris/MediaPlayer2", T);
        let _: Result<(), _> = p.method_call(PLAYER, method, ());
    }
}

pub fn seek_to(app: &mut App, frac: f32) {
    let name = app.svc.bus.active.clone();
    let m = &app.st.media;
    if name.is_empty() || m.length_us <= 0 {
        return;
    }
    let pos = (m.length_us as f64 * frac as f64) as i64;
    let track = m.track_id.clone();
    if let Some(c) = app.svc.bus.session.clone() {
        let p = c.with_proxy(name.as_str(), "/org/mpris/MediaPlayer2", T);
        let ok = Path::new(track.clone()).ok().map(|tp| {
            let r: Result<(), _> = p.method_call(PLAYER, "SetPosition", (tp, pos));
            r.is_ok()
        });
        if ok != Some(true) {
            let off = pos - app.st.media.position(Instant::now()) as i64 * 1_000_000;
            let _: Result<(), _> = p.method_call(PLAYER, "Seek", (off,));
        }
    }
    app.st.media.pos_us = pos;
    app.st.media.pos_at = Some(Instant::now());
    app.mark(topic::MEDIA);
}

pub fn seek_rel(app: &mut App, secs: f64) {
    let name = app.svc.bus.active.clone();
    if let Some(c) = app.svc.bus.session.clone() {
        let p = c.with_proxy(name.as_str(), "/org/mpris/MediaPlayer2", T);
        let _: Result<(), _> = p.method_call(PLAYER, "Seek", ((secs * 1e6) as i64,));
    }
}

// ------------------------------------------------------------------ notifications server

fn own_notifications(app: &mut App) {
    let Some(c) = app.svc.bus.session.clone() else { return };
    match c.request_name(NOTIF_IFACE, true, true, true) {
        Ok(dbus::blocking::stdintf::org_freedesktop_dbus::RequestNameReply::PrimaryOwner)
        | Ok(dbus::blocking::stdintf::org_freedesktop_dbus::RequestNameReply::AlreadyOwner) => {
            app.st.notifs.owned = true;
        }
        _ => {
            eprintln!("dshell: another notification daemon is running; dshell will not show notifications");
        }
    }
}

const INTROSPECT: &str = r#"<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
<node><interface name="org.freedesktop.Notifications">
<method name="GetCapabilities"><arg direction="out" type="as"/></method>
<method name="Notify"><arg type="s" direction="in"/><arg type="u" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="as" direction="in"/><arg type="a{sv}" direction="in"/><arg type="i" direction="in"/><arg type="u" direction="out"/></method>
<method name="CloseNotification"><arg type="u" direction="in"/></method>
<method name="GetServerInformation"><arg type="s" direction="out"/><arg type="s" direction="out"/><arg type="s" direction="out"/><arg type="s" direction="out"/></method>
<signal name="NotificationClosed"><arg type="u"/><arg type="u"/></signal>
<signal name="ActionInvoked"><arg type="u"/><arg type="s"/></signal>
</interface></node>"#;

fn notif_call(app: &mut App, c: &Rc<LocalConnection>, m: &Message) {
    let member = m.member().map(|s| s.to_string()).unwrap_or_default();
    let reply = match member.as_str() {
        "GetCapabilities" => {
            Some(m.method_return().append1(vec!["actions", "body", "icon-static", "persistence", "body-hyperlinks"]))
        }
        "GetServerInformation" => Some(m.method_return().append2("dshell", "DarkShadow").append2(env!("CARGO_PKG_VERSION"), "1.2")),
        "Introspect" => Some(m.method_return().append1(INTROSPECT)),
        "CloseNotification" => {
            let id: u32 = m.read1().unwrap_or(0);
            close(app, id, 3);
            Some(m.method_return())
        }
        "Notify" => {
            let id = notify(app, m);
            Some(m.method_return().append1(id))
        }
        _ => Some(unknown(m)),
    };
    if let Some(r) = reply {
        let _ = c.channel().send(r);
        c.channel().flush();
    }
}

fn unknown(m: &Message) -> Message {
    m.error(&"org.freedesktop.DBus.Error.UnknownMethod".into(), c"unknown method")
}

fn hint_str(h: &PropMap, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| h.get(*k).and_then(|v| v.0.as_str().map(str::to_string)))
}

fn notify(app: &mut App, m: &Message) -> u32 {
    let mut it = m.iter_init();
    let app_name: String = it.read().unwrap_or_default();
    let replaces: u32 = it.read().unwrap_or(0);
    let icon: String = it.read().unwrap_or_default();
    let summary: String = it.read().unwrap_or_default();
    let body: String = it.read().unwrap_or_default();
    let actions: Vec<String> = it.read().unwrap_or_default();
    let hints: PropMap = it.read().unwrap_or_default();
    let timeout: i32 = it.read().unwrap_or(-1);
    let n = &mut app.st.notifs;
    let id = if replaces != 0 && n.list.iter().any(|x| x.id == replaces) {
        replaces
    } else {
        n.next_id = n.next_id.wrapping_add(1).max(1);
        n.next_id
    };
    let urgency = hints.get("urgency").and_then(|v| v.0.as_u64()).unwrap_or(1) as u8;
    let mut pairs = Vec::new();
    for ch in actions.chunks(2) {
        if ch.len() == 2 && ch[0] != "default" {
            pairs.push((ch[0].clone(), ch[1].clone()));
        } else if ch.len() == 2 {
            pairs.insert(0, (ch[0].clone(), String::new()));
        }
    }
    let notif = Notif {
        id,
        app: app_name,
        summary,
        body: strip_markup(&body),
        icon: icon.clone(),
        image: None,
        actions: pairs,
        urgency,
        timeout_ms: timeout,
        at: Instant::now(),
        wall: std::time::SystemTime::now(),
    };
    let n = &mut app.st.notifs;
    n.list.retain(|x| x.id != id);
    n.list.insert(0, notif);
    let cap = app.cfg.notifications.history.max(5) as usize;
    n.list.truncate(cap);
    // the picture: raw pixels in the hints, a path, or the app icon
    let raw = hints.get("image-data").or_else(|| hints.get("image_data")).or_else(|| hints.get("icon_data"));
    if let Some(img) = raw.and_then(|v| raw_image(&*v.0)) {
        if let Some(x) = app.st.notifs.list.iter_mut().find(|x| x.id == id) {
            x.image = Some(Rc::new(img));
        }
    } else {
        let path = hint_str(&hints, &["image-path", "image_path"]).unwrap_or(icon);
        if !path.is_empty() {
            crate::svc::images::load_notif_icon(app, id, path);
        }
    }
    crate::actions::notif_arrived(app, id);
    id
}

fn raw_image(v: &dyn RefArg) -> Option<crate::gfx::Image> {
    let items: Vec<&dyn RefArg> = unwrap_variant(v).as_iter()?.collect();
    if items.len() < 7 {
        return None;
    }
    let w = items[0].as_i64()? as u32;
    let h = items[1].as_i64()? as u32;
    let stride = items[2].as_i64()? as usize;
    let alpha = items[3].as_i64().map(|x| x != 0).or_else(|| items[3].as_u64().map(|x| x != 0)).unwrap_or(true);
    let chans = items[5].as_i64()? as usize;
    let data: Vec<u8> = items[6].as_iter()?.map(|b| b.as_u64().unwrap_or(0) as u8).collect();
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as usize {
        for x in 0..w as usize {
            let i = y * stride + x * chans;
            if i + chans > data.len() {
                return None;
            }
            rgba.extend_from_slice(&[data[i], data[i + 1], data[i + 2], if alpha && chans == 4 { data[i + 3] } else { 255 }]);
        }
    }
    let img = image::RgbaImage::from_raw(w, h, rgba)?;
    let side = 96;
    let img = if w > side || h > side { image::imageops::thumbnail(&img, side, side * h / w.max(1)) } else { img };
    crate::gfx::Image::from_rgba(img.width(), img.height(), img.into_raw())
}

fn strip_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut tag = false;
    for c in s.chars() {
        match c {
            '<' => tag = true,
            '>' if tag => tag = false,
            _ if !tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'")
}

/// reason: 1 expired, 2 dismissed, 3 closed by a call
pub fn close(app: &mut App, id: u32, reason: u32) {
    let n = &mut app.st.notifs;
    let had = n.list.iter().any(|x| x.id == id);
    n.list.retain(|x| x.id != id);
    if n.popup == Some(id) {
        n.popup = None;
    }
    if had {
        emit(app, "NotificationClosed", id, Some(reason), None);
    }
    app.mark(topic::NOTIF);
}

pub fn invoke(app: &mut App, id: u32, key: &str) {
    emit(app, "ActionInvoked", id, None, Some(key));
    close(app, id, 2);
}

fn emit(app: &mut App, sig: &str, id: u32, reason: Option<u32>, key: Option<&str>) {
    let Some(c) = app.svc.bus.session.clone() else { return };
    if let Ok(m) = Message::new_signal(NOTIF_PATH, NOTIF_IFACE, sig) {
        let m = match (reason, key) {
            (Some(r), _) => m.append2(id, r),
            (_, Some(k)) => m.append2(id, k),
            _ => m.append1(id),
        };
        let _ = c.channel().send(m);
        c.channel().flush();
    }
}

// ------------------------------------------------------------------ system bus

fn on_system(app: &mut App, m: &Message) {
    if m.msg_type() != MessageType::Signal {
        return;
    }
    let path = m.path().map(|p| p.to_string()).unwrap_or_default();
    if path.starts_with("/org/freedesktop/NetworkManager") {
        arm_net(app);
    } else if path.starts_with("/org/bluez") || m.interface().as_deref() == Some("org.freedesktop.DBus.ObjectManager") {
        arm_bt(app);
    } else if path.starts_with("/org/freedesktop/UPower") {
        refresh_power(app);
    }
}

fn arm_net(app: &mut App) {
    if app.svc.bus.net_armed {
        return;
    }
    app.svc.bus.net_armed = true;
    let _ = app.lh.insert_source(Timer::from_duration(Duration::from_millis(150)), |_, _, app: &mut App| {
        app.svc.bus.net_armed = false;
        refresh_net(app);
        TimeoutAction::Drop
    });
}

fn arm_bt(app: &mut App) {
    if app.svc.bus.bt_armed {
        return;
    }
    app.svc.bus.bt_armed = true;
    let _ = app.lh.insert_source(Timer::from_duration(Duration::from_millis(150)), |_, _, app: &mut App| {
        app.svc.bus.bt_armed = false;
        refresh_bt(app);
        TimeoutAction::Drop
    });
}

pub fn refresh_net(app: &mut App) {
    let Some(c) = app.svc.bus.system.clone() else { return };
    let nm = c.with_proxy("org.freedesktop.NetworkManager", "/org/freedesktop/NetworkManager", T);
    let Ok(props) = nm.get_all("org.freedesktop.NetworkManager") else {
        app.st.net.ok = false;
        return;
    };
    let net = &mut app.st.net;
    net.ok = true;
    net.wifi_on = props.get("WirelessEnabled").and_then(|v| v.0.as_u64()).map(|x| x != 0).unwrap_or(false);
    let ptype = props.get("PrimaryConnectionType").and_then(|v| v.0.as_str()).unwrap_or("").to_string();
    let primary = props.get("PrimaryConnection").and_then(|v| v.0.as_str()).unwrap_or("/").to_string();
    net.kind = match ptype.as_str() {
        "802-11-wireless" => NetKind::Wifi,
        "802-3-ethernet" | "bridge" | "bond" | "vlan" => NetKind::Wired,
        _ => NetKind::None,
    };
    net.ssid.clear();
    if net.kind == NetKind::Wifi && primary != "/" {
        let ac = c.with_proxy("org.freedesktop.NetworkManager", primary.as_str(), T);
        if let Ok(id) = ac.get::<String>("org.freedesktop.NetworkManager.Connection.Active", "Id") {
            app.st.net.ssid = id;
        }
        if let Ok(ap) = ac.get::<Path>("org.freedesktop.NetworkManager.Connection.Active", "SpecificObject") {
            let ap = ap.to_string();
            let p = c.with_proxy("org.freedesktop.NetworkManager", ap.as_str(), T);
            if let Ok(s) = p.get::<u8>("org.freedesktop.NetworkManager.AccessPoint", "Strength") {
                app.st.net.strength = s;
            }
            app.svc.bus.nm_ap = ap;
        }
    }
    app.mark(topic::NET);
}

pub fn set_wifi(app: &mut App, on: bool) {
    let Some(c) = app.svc.bus.system.clone() else { return };
    let nm = c.with_proxy("org.freedesktop.NetworkManager", "/org/freedesktop/NetworkManager", T);
    let _ = nm.set("org.freedesktop.NetworkManager", "WirelessEnabled", on);
    app.st.net.wifi_on = on;
    app.mark(topic::NET);
}

pub fn refresh_bt(app: &mut App) {
    let Some(c) = app.svc.bus.system.clone() else { return };
    let p = c.with_proxy("org.bluez", "/", T);
    type Objs = HashMap<Path<'static>, HashMap<String, PropMap>>;
    let r: Result<(Objs,), _> = p.method_call("org.freedesktop.DBus.ObjectManager", "GetManagedObjects", ());
    let bt = &mut app.st.bt;
    bt.present = false;
    bt.powered = false;
    bt.connected.clear();
    if let Ok((objs,)) = r {
        for (_, ifs) in objs {
            if let Some(a) = ifs.get("org.bluez.Adapter1") {
                bt.present = true;
                if a.get("Powered").and_then(|v| v.0.as_u64()).unwrap_or(0) != 0 {
                    bt.powered = true;
                }
            }
            if let Some(d) = ifs.get("org.bluez.Device1")
                && d.get("Connected").and_then(|v| v.0.as_u64()).unwrap_or(0) != 0
            {
                bt.connected.push(d.get("Alias").and_then(|v| v.0.as_str()).unwrap_or("Device").to_string());
            }
        }
    }
    app.mark(topic::BT);
}

pub fn set_bt(app: &mut App, on: bool) {
    let Some(c) = app.svc.bus.system.clone() else { return };
    let p = c.with_proxy("org.bluez", "/", T);
    type Objs = HashMap<Path<'static>, HashMap<String, PropMap>>;
    let r: Result<(Objs,), _> = p.method_call("org.freedesktop.DBus.ObjectManager", "GetManagedObjects", ());
    if let Ok((objs,)) = r {
        for (path, ifs) in objs {
            if ifs.contains_key("org.bluez.Adapter1") {
                let a = c.with_proxy("org.bluez", path, T);
                let _ = a.set("org.bluez.Adapter1", "Powered", on);
            }
        }
    }
    if on && crate::proc::which("rfkill") {
        crate::proc::spawn(&["rfkill", "unblock", "bluetooth"]);
    }
    app.st.bt.powered = on;
    app.mark(topic::BT);
}

pub fn refresh_power(app: &mut App) {
    let Some(c) = app.svc.bus.system.clone() else { return };
    let p = c.with_proxy("org.freedesktop.UPower", "/org/freedesktop/UPower/devices/DisplayDevice", T);
    let Ok(props) = p.get_all("org.freedesktop.UPower.Device") else { return };
    let up = c.with_proxy("org.freedesktop.UPower", "/org/freedesktop/UPower", T);
    let on_batt = up.get::<bool>("org.freedesktop.UPower", "OnBattery").unwrap_or(false);
    let b = &mut app.st.batt;
    let old_plug = b.plugged;
    let was_present = b.present;
    b.present = props.get("IsPresent").and_then(|v| v.0.as_u64()).map(|x| x != 0).unwrap_or(false)
        && props.get("Type").and_then(|v| v.0.as_u64()).unwrap_or(0) == 2;
    b.pct = props.get("Percentage").and_then(|v| v.0.as_f64()).unwrap_or(0.0) as f32;
    let state = props.get("State").and_then(|v| v.0.as_u64()).unwrap_or(0);
    b.charging = state == 1;
    b.plugged = !on_batt;
    let changed = was_present && b.present && old_plug != b.plugged;
    app.mark(topic::BATT);
    if changed && app.cfg.island.charger {
        crate::actions::island_flash(app, crate::ui::IslandMode::Charger);
    }
}

/// logind sets the backlight without root (what brightnessctl does too).
pub fn set_backlight(app: &mut App, device: &str, value: u32) -> bool {
    let Some(c) = app.svc.bus.system.clone() else { return false };
    let p = c.with_proxy("org.freedesktop.login1", "/org/freedesktop/login1/session/auto", T);
    let r: Result<(), _> = p.method_call("org.freedesktop.login1.Session", "SetBrightness", ("backlight", device, value));
    r.is_ok()
}

#[allow(dead_code)]
fn _variant_unused(_: Variant<u8>) {}
