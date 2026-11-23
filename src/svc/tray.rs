//! The system tray (StatusNotifierItem). dshell is the watcher (when no one else is) and the host: apps
//! register their items, the bar shows their icons, and clicks go back to the apps (Activate, ContextMenu,
//! SecondaryActivate).

use std::rc::Rc;
use std::time::Duration;

use dbus::arg::{PropMap, RefArg};
use dbus::blocking::LocalConnection;
use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
use dbus::Message;

use crate::app::App;
use crate::gfx::Image;
use crate::state::topic;

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM: &str = "org.kde.StatusNotifierItem";
const T: Duration = Duration::from_millis(600);

#[derive(Clone)]
pub struct Item {
    /// bus name the item lives on (unique or well-known)
    pub service: String,
    pub path: String,
    pub id: String,
    pub title: String,
    pub status: String,
    pub icon: Option<Rc<Image>>,
    pub icon_name: String,
    /// the dbusmenu object of the item ("" or "/" when it has none)
    pub menu: String,
}

/// One line of a tray menu.
#[derive(Clone, Debug)]
pub struct MenuEntry {
    pub id: i32,
    pub label: String,
    pub depth: u8,
    pub enabled: bool,
    pub separator: bool,
    /// None: no check; Some(on)
    pub check: Option<bool>,
    pub submenu: bool,
}

#[derive(Default)]
pub struct Tray {
    /// we own the watcher name (else someone else's watcher tells us about items)
    pub watcher: bool,
    pub items: Vec<Item>,
}

pub fn start(app: &mut App, c: &Rc<LocalConnection>) {
    let _ = c.add_match_no_cb("type='signal',interface='org.kde.StatusNotifierItem'");
    let _ = c.add_match_no_cb("type='signal',interface='org.kde.StatusNotifierWatcher'");
    let _ = c.add_match_no_cb("type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged'");
    use dbus::blocking::stdintf::org_freedesktop_dbus::RequestNameReply::*;
    app.svc.tray.watcher = matches!(c.request_name(WATCHER, false, false, true), Ok(PrimaryOwner) | Ok(AlreadyOwner));
    let host = format!("org.kde.StatusNotifierHost-{}", std::process::id());
    let _ = c.request_name(host.as_str(), false, false, true);
    if !app.svc.tray.watcher {
        // another watcher (waybar, KDE...): register with it and take its list
        let w = c.with_proxy(WATCHER, WATCHER_PATH, T);
        let _: Result<(), _> = w.method_call(WATCHER, "RegisterStatusNotifierHost", (host.as_str(),));
        if let Ok(list) = w.get::<Vec<String>>(WATCHER, "RegisteredStatusNotifierItems") {
            for s in list {
                add(app, c, &s, "");
            }
        }
    } else if let Ok(m) = Message::new_signal(WATCHER_PATH, WATCHER, "StatusNotifierHostRegistered") {
        let _ = c.channel().send(m);
    }
}

/// "service" or "service/path" or a bare path (then the sender is the service).
fn split(arg: &str, sender: &str) -> (String, String) {
    if arg.starts_with('/') {
        return (sender.to_string(), arg.to_string());
    }
    match arg.find('/') {
        Some(i) => (arg[..i].to_string(), arg[i..].to_string()),
        None => (arg.to_string(), "/StatusNotifierItem".to_string()),
    }
}

fn add(app: &mut App, c: &Rc<LocalConnection>, arg: &str, sender: &str) {
    let (service, path) = split(arg, sender);
    if service.is_empty() || app.svc.tray.items.iter().any(|i| i.service == service && i.path == path) {
        return;
    }
    let item = Item {
        service: service.clone(),
        path: path.clone(),
        id: String::new(),
        title: String::new(),
        status: String::new(),
        icon: None,
        icon_name: String::new(),
        menu: String::new(),
    };
    app.svc.tray.items.push(item);
    // read it a moment later: an app that registers synchronously cannot answer us before our reply
    // reaches it, and some only finish setting up after registering
    schedule_read(app, service.clone(), path.clone(), 0);
    if app.svc.tray.watcher
        && let Ok(m) = Message::new_signal(WATCHER_PATH, WATCHER, "StatusNotifierItemRegistered")
    {
        let _ = c.channel().send(m.append1(format!("{service}{path}")));
    }
    publish(app);
}

fn schedule_read(app: &mut App, service: String, path: String, tries: u32) {
    use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
    let delay = if tries == 0 { 40 } else { 500 };
    let _ = app.lh.insert_source(Timer::from_duration(Duration::from_millis(delay)), move |_, _, app: &mut App| {
        let Some(c) = app.svc.bus.session.clone() else { return TimeoutAction::Drop };
        if let Some(i) = app.svc.tray.items.iter().position(|i| i.service == service && i.path == path) {
            let mut it = app.svc.tray.items[i].clone();
            let ok = read(app, &c, &mut it);
            if let Some(slot) = app.svc.tray.items.iter_mut().find(|i| i.service == service && i.path == path) {
                *slot = it;
            }
            publish(app);
            if !ok && tries < 4 {
                schedule_read(app, service.clone(), path.clone(), tries + 1);
            }
        }
        TimeoutAction::Drop
    });
}

fn read(app: &mut App, c: &Rc<LocalConnection>, item: &mut Item) -> bool {
    let p = c.with_proxy(item.service.as_str(), item.path.as_str(), T);
    let Ok(props) = p.get_all(ITEM) else { return false };
    let s = |k: &str| props.get(k).and_then(|v| v.0.as_str()).unwrap_or("").to_string();
    item.id = s("Id");
    item.title = s("Title");
    item.status = s("Status");
    item.icon_name = s("IconName");
    item.menu = props.get("Menu").and_then(|v| v.0.as_str().map(str::to_string)).unwrap_or_default();
    let theme_path = s("IconThemePath");
    item.icon = None;
    // a named icon from the theme first (sharper), else the pixels the app sends
    if !item.icon_name.is_empty() {
        let found = if !theme_path.is_empty() {
            ["svg", "png"].iter().map(|e| std::path::Path::new(&theme_path).join(format!("{}.{e}", item.icon_name))).find(|p| p.is_file())
        } else {
            None
        };
        let theme = app.cfg.apps.icon_theme.clone();
        let path = found.or_else(|| app.svc.apps.find_icon(&item.icon_name, &theme));
        if let Some(path) = path {
            let key = (item.service.clone(), item.path.clone());
            crate::svc::images::load_square(app, path.to_string_lossy().into_owned(), 64, move |app, img| {
                if let Some(it) = app.svc.tray.items.iter_mut().find(|i| i.service == key.0 && i.path == key.1) {
                    it.icon = img;
                }
                publish(app);
            });
            return true;
        }
    }
    if let Some(v) = props.get("IconPixmap") {
        item.icon = pixmap(&*v.0).map(Rc::new);
    }
    true
}

/// a(iiay): the biggest of the sizes the app sent, ARGB32 in network byte order.
fn pixmap(v: &dyn RefArg) -> Option<Image> {
    let mut best: Option<(u32, u32, Vec<u8>)> = None;
    let v = crate::svc::bus::unwrap_variant(v);
    for entry in v.as_iter()? {
        let entry = crate::svc::bus::unwrap_variant(entry);
        let parts: Vec<&dyn RefArg> = entry.as_iter()?.collect();
        if parts.len() < 3 {
            continue;
        }
        let w = parts[0].as_i64()? as u32;
        let h = parts[1].as_i64()? as u32;
        let data: Vec<u8> = parts[2].as_iter()?.filter_map(|b| b.as_u64().map(|x| x as u8)).collect();
        if w == 0 || h == 0 || data.len() < (w * h * 4) as usize || w > 512 {
            continue;
        }
        if best.as_ref().map(|b| w > b.0).unwrap_or(true) {
            best = Some((w, h, data));
        }
    }
    let (w, h, data) = best?;
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for px in data.chunks_exact(4).take((w * h) as usize) {
        rgba.extend_from_slice(&[px[1], px[2], px[3], px[0]]);
    }
    Image::from_rgba(w, h, rgba)
}

fn publish(app: &mut App) {
    // passive items stay out of the bar, like everywhere else
    app.st.tray = app
        .svc
        .tray
        .items
        .iter()
        .filter(|i| i.status != "Passive")
        .map(|i| crate::state::TrayIcon {
            key: format!("{}{}", i.service, i.path),
            title: if i.title.is_empty() { i.id.clone() } else { i.title.clone() },
            icon: i.icon.clone(),
            letter: i.id.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default(),
        })
        .collect();
    app.mark(topic::TRAY);
}

/// Method calls to our watcher. Returns true when handled.
pub fn call(app: &mut App, c: &Rc<LocalConnection>, m: &Message) -> bool {
    if m.path().as_deref() != Some(WATCHER_PATH) || !app.svc.tray.watcher {
        return false;
    }
    let member = m.member().map(|s| s.to_string()).unwrap_or_default();
    let iface = m.interface().map(|s| s.to_string()).unwrap_or_default();
    let sender = m.sender().map(|s| s.to_string()).unwrap_or_default();
    let reply = match (iface.as_str(), member.as_str()) {
        (_, "RegisterStatusNotifierItem") => {
            let arg: String = m.read1().unwrap_or_default();
            let r = m.method_return();
            let _ = c.channel().send(r);
            add(app, c, &arg, &sender);
            return true;
        }
        (_, "RegisterStatusNotifierHost") => m.method_return(),
        ("org.freedesktop.DBus.Properties", "Get") => {
            let (_, prop): (String, String) = m.read2().unwrap_or_default();
            match prop.as_str() {
                "RegisteredStatusNotifierItems" => m.method_return().append1(dbus::arg::Variant(registered(app))),
                "IsStatusNotifierHostRegistered" => m.method_return().append1(dbus::arg::Variant(true)),
                "ProtocolVersion" => m.method_return().append1(dbus::arg::Variant(0i32)),
                _ => m.error(&"org.freedesktop.DBus.Error.UnknownProperty".into(), c"unknown property"),
            }
        }
        ("org.freedesktop.DBus.Properties", "GetAll") => {
            let mut p = PropMap::new();
            p.insert("RegisteredStatusNotifierItems".into(), dbus::arg::Variant(Box::new(registered(app))));
            p.insert("IsStatusNotifierHostRegistered".into(), dbus::arg::Variant(Box::new(true)));
            p.insert("ProtocolVersion".into(), dbus::arg::Variant(Box::new(0i32)));
            m.method_return().append1(p)
        }
        ("org.freedesktop.DBus.Introspectable", "Introspect") => m.method_return().append1(INTROSPECT),
        _ => m.error(&"org.freedesktop.DBus.Error.UnknownMethod".into(), c"unknown method"),
    };
    let _ = c.channel().send(reply);
    c.channel().flush();
    true
}

fn registered(app: &App) -> Vec<String> {
    app.svc.tray.items.iter().map(|i| format!("{}{}", i.service, i.path)).collect()
}

const INTROSPECT: &str = r#"<node><interface name="org.kde.StatusNotifierWatcher">
<method name="RegisterStatusNotifierItem"><arg type="s" direction="in"/></method>
<method name="RegisterStatusNotifierHost"><arg type="s" direction="in"/></method>
<property name="RegisteredStatusNotifierItems" type="as" access="read"/>
<property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
<property name="ProtocolVersion" type="i" access="read"/>
<signal name="StatusNotifierItemRegistered"><arg type="s"/></signal>
<signal name="StatusNotifierItemUnregistered"><arg type="s"/></signal>
<signal name="StatusNotifierHostRegistered"/>
</interface></node>"#;

/// Signals: an item changed, a watcher (not us) announced one, or a name went away.
pub fn signal(app: &mut App, c: &Rc<LocalConnection>, m: &Message) {
    let member = m.member().map(|s| s.to_string()).unwrap_or_default();
    let iface = m.interface().map(|s| s.to_string()).unwrap_or_default();
    let sender = m.sender().map(|s| s.to_string()).unwrap_or_default();
    if iface == ITEM {
        // NewIcon, NewStatus, NewTitle, NewToolTip ... from an item: read it again
        let path = m.path().map(|p| p.to_string()).unwrap_or_default();
        let idx = app.svc.tray.items.iter().position(|i| (i.service == sender || owner_of(c, &i.service) == sender) && i.path == path);
        if let Some(i) = idx {
            let mut it = app.svc.tray.items[i].clone();
            let _ = read(app, c, &mut it);
            if let Some(slot) = app.svc.tray.items.get_mut(i) {
                *slot = it;
            }
            publish(app);
        }
        return;
    }
    if iface == WATCHER && !app.svc.tray.watcher {
        let arg: String = m.read1().unwrap_or_default();
        match member.as_str() {
            "StatusNotifierItemRegistered" => add(app, c, &arg, ""),
            "StatusNotifierItemUnregistered" => {
                let (s, p) = split(&arg, "");
                app.svc.tray.items.retain(|i| !(i.service == s && i.path == p));
                publish(app);
            }
            _ => {}
        }
        return;
    }
    if member == "NameOwnerChanged" {
        let (name, _old, new): (String, String, String) = m.read3().unwrap_or_default();
        if new.is_empty() {
            let before = app.svc.tray.items.len();
            let gone: Vec<String> = app.svc.tray.items.iter().filter(|i| i.service == name).map(|i| format!("{}{}", i.service, i.path)).collect();
            app.svc.tray.items.retain(|i| i.service != name);
            if app.svc.tray.items.len() != before {
                if app.svc.tray.watcher {
                    for g in gone {
                        if let Ok(s) = Message::new_signal(WATCHER_PATH, WATCHER, "StatusNotifierItemUnregistered") {
                            let _ = c.channel().send(s.append1(g));
                        }
                    }
                }
                publish(app);
            }
        }
    }
}

fn owner_of(c: &Rc<LocalConnection>, name: &str) -> String {
    if name.starts_with(':') {
        return name.to_string();
    }
    let p = c.with_proxy("org.freedesktop.DBus", "/org/freedesktop/DBus", T);
    let r: Result<(String,), _> = p.method_call("org.freedesktop.DBus", "GetNameOwner", (name,));
    r.map(|x| x.0).unwrap_or_default()
}

/// A click on an icon: 1 left (Activate), 2 right (ContextMenu), 3 middle (SecondaryActivate).
pub fn click(app: &mut App, key: &str, button: u8, x: i32, y: i32) {
    let Some(c) = app.svc.bus.session.clone() else { return };
    let Some(it) = app.svc.tray.items.iter().find(|i| format!("{}{}", i.service, i.path) == key).cloned() else { return };
    // the right button: our own menu from the app's dbusmenu, when it has one
    if button == 2 && !it.menu.is_empty() && it.menu != "/" {
        let entries = menu_layout(&c, &it.service, &it.menu);
        if !entries.is_empty() {
            crate::actions::open_tray_menu(app, it.service.clone(), it.menu.clone(), entries, x);
            return;
        }
    }
    let method = match button {
        2 => "ContextMenu",
        3 => "SecondaryActivate",
        _ => "Activate",
    };
    if let Ok(m) = Message::new_method_call(it.service.as_str(), it.path.as_str(), ITEM, method) {
        let _ = c.channel().send(m.append2(x, y));
        c.channel().flush();
    }
}


fn label_of(props: &PropMap) -> String {
    // "_File" -> "File" (mnemonics), "__" -> "_"
    let raw = props.get("label").and_then(|v| v.0.as_str()).unwrap_or("");
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '_' {
            if chars.peek() == Some(&'_') {
                out.push('_');
                chars.next();
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn walk(node: &dyn RefArg, depth: u8, out: &mut Vec<MenuEntry>) {
    let node = crate::svc::bus::unwrap_variant(node);
    let Some(parts) = node.as_iter().map(|i| i.collect::<Vec<&dyn RefArg>>()) else { return };
    if parts.len() < 3 {
        return;
    }
    let id = parts[0].as_i64().unwrap_or(0) as i32;
    let mut props = PropMap::new();
    if let Some(mut it) = parts[1].as_iter() {
        while let (Some(k), Some(v)) = (it.next(), it.next()) {
            if let Some(k) = k.as_str() {
                props.insert(k.to_string(), dbus::arg::Variant(crate::svc::bus::unwrap_variant(v).box_clone()));
            }
        }
    }
    let flag = |k: &str, d: bool| props.get(k).and_then(|v| v.0.as_u64().or_else(|| v.0.as_i64().map(|x| x as u64))).map(|x| x != 0).unwrap_or(d);
    let children: Vec<&dyn RefArg> = parts[2].as_iter().map(|i| i.collect()).unwrap_or_default();
    if depth > 0 {
        if !flag("visible", true) {
            return;
        }
        let separator = props.get("type").and_then(|v| v.0.as_str()) == Some("separator");
        let check = match props.get("toggle-type").and_then(|v| v.0.as_str()) {
            Some("checkmark") | Some("radio") => Some(props.get("toggle-state").and_then(|v| v.0.as_i64()).unwrap_or(0) == 1),
            _ => None,
        };
        out.push(MenuEntry {
            id,
            label: label_of(&props),
            depth: depth - 1,
            enabled: flag("enabled", true),
            separator,
            check,
            submenu: !children.is_empty(),
        });
    }
    if depth < 3 {
        for ch in children {
            walk(ch, depth + 1, out);
        }
    }
}

/// The app's menu, flattened (submenus indented below their parent).
pub fn menu_layout(c: &Rc<LocalConnection>, service: &str, path: &str) -> Vec<MenuEntry> {
    let p = c.with_proxy(service, path, T);
    let _: Result<(bool,), _> = p.method_call("com.canonical.dbusmenu", "AboutToShow", (0i32,));
    let r: Result<Message, _> = (|| {
        let m = Message::new_method_call(service, path, "com.canonical.dbusmenu", "GetLayout")?
            .append3(0i32, -1i32, Vec::<String>::new());
        c.channel().send_with_reply_and_block(m, T).map_err(|e| e.to_string())
    })();
    let mut out = Vec::new();
    if let Ok(reply) = r {
        let mut it = reply.iter_init();
        it.next(); // revision
        if let Some(layout) = it.get_refarg() {
            walk(&*layout, 0, &mut out);
        }
    }
    // no separators at the ends or twice in a row
    let mut clean: Vec<MenuEntry> = Vec::new();
    for e in out {
        if e.separator && clean.last().map(|l| l.separator).unwrap_or(true) {
            continue;
        }
        clean.push(e);
    }
    while clean.last().map(|l| l.separator).unwrap_or(false) {
        clean.pop();
    }
    clean
}

/// A menu line was picked.
pub fn menu_event(app: &mut App, service: &str, path: &str, id: i32) {
    let Some(c) = app.svc.bus.session.clone() else { return };
    if let Ok(m) = Message::new_method_call(service, path, "com.canonical.dbusmenu", "Event") {
        let ts = crate::clock::unix() as u32;
        let _ = c.channel().send(m.append3(id, "clicked", dbus::arg::Variant(0i32)).append1(ts));
        c.channel().flush();
    }
}
