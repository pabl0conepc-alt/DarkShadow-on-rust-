//! What happens: clicks, keys, the CLI, notifications arriving, the island reacting. The UI modules only draw.

use std::time::{Duration, Instant};

use smithay_client_toolkit::{
    reexports::calloop::timer::{TimeoutAction, Timer},
    seat::keyboard::{KeyEvent, Keysym},
    shell::{
        WaylandSurface,
        wlr_layer::{Anchor, KeyboardInteractivity, Layer},
    },
};

use crate::app::{App, Spec, make_theme};
use crate::shell::SHADOW;
use crate::state::{WmKind, topic};
use crate::svc::{audio, bus, drift, hypr};
use crate::ui::{Act, CardMode, IslandMode, Kind, PowerAct, WidgetId};

// ------------------------------------------------------------------ panels

fn bar_top(app: &App) -> bool {
    app.cfg.bar.position == "top"
}

pub fn open_card(app: &mut App, mode: CardMode, pinned: bool) {
    let now = Instant::now();
    let c = &mut app.ui.card;
    if app.surfs.iter().any(|s| s.kind == Kind::Card && !s.closing) && c.mode == mode && pinned && c.pinned {
        close_card(app);
        return;
    }
    c.mode = mode;
    c.tab = if mode == CardMode::System { 1 } else { 0 };
    c.pinned = pinned || c.pinned && c.rev.open;
    c.rev.set(true, now);
    c.left_at = None;
    let top = bar_top(app);
    let bh = if app.cfg.bar.enabled { crate::shell::bar_height(app) as i32 - 10 } else { 8 };
    app.open(Spec {
        kind: Kind::Card,
        layer: Layer::Top,
        ns: "dshell-island",
        anchor: if top { Anchor::TOP } else { Anchor::BOTTOM },
        size: (520 + 2 * SHADOW as u32, 330 + 2 * SHADOW as u32),
        exclusive: -1,
        margin: if top { (bh, 0, 0, 0) } else { (0, 0, bh, 0) },
        kb: if pinned { KeyboardInteractivity::OnDemand } else { KeyboardInteractivity::None },
        output: app.svc.main_output.clone(),
        topics: topic::CLOCK | topic::MEDIA | topic::SYS | topic::WEATHER | topic::NOTIF | topic::THEME,
        fps: 30,
    });
    let pinned_now = app.ui.card.pinned;
    if let Some(s) = app.surf_mut(Kind::Card) {
        s.layer.set_keyboard_interactivity(if pinned_now {
            KeyboardInteractivity::OnDemand
        } else {
            KeyboardInteractivity::None
        });
        s.layer.commit();
    }
    crate::svc::sysmon::update_wanted(app);
    if mode == CardMode::Today {
        crate::svc::weather::fetch(app);
    }
    app.mark_kind(Kind::Card);
    app.mark_kind(Kind::Bar);
}

pub fn close_card(app: &mut App) {
    if !app.has(Kind::Card) {
        return;
    }
    app.ui.card.rev.set(false, Instant::now());
    app.ui.card.pinned = false;
    app.ui.card.notif = None;
    app.mark_kind(Kind::Card);
    app.mark_kind(Kind::Bar);
    crate::svc::sysmon::update_wanted(app);
}

pub fn open_launcher(app: &mut App) {
    if app.has(Kind::Launcher) && !app.ui.launcher.clip_mode {
        close_launcher(app);
        return;
    }
    app.destroy(Kind::Launcher);
    app.ui.launcher.clip_mode = false;
    app.ui.launcher.clip.clear();
    show_launcher(app);
}

/// The clipboard history (cliphist), in the launcher's window.
pub fn open_clipboard(app: &mut App) {
    if app.has(Kind::Launcher) && app.ui.launcher.clip_mode {
        close_launcher(app);
        return;
    }
    if !crate::proc::which("cliphist") {
        system_notice(app, "Clipboard", "cliphist is not installed (it keeps the history)");
        return;
    }
    crate::proc::capture(app, &["cliphist", "list"], |app, out| {
        app.ui.launcher.clip = out
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(300)
            .map(|l| {
                let shown = l.split_once('\t').map(|x| x.1).unwrap_or(l).trim().to_string();
                (l.to_string(), shown)
            })
            .collect();
        app.destroy(Kind::Launcher);
        app.ui.launcher.clip_mode = true;
        show_launcher(app);
    });
}

fn show_launcher(app: &mut App) {
    close_others(app, Kind::Launcher);
    app.svc.apps.ensure();
    let l = &mut app.ui.launcher;
    l.query.clear();
    l.sel = 0;
    l.top = 0;
    l.rev.set(true, Instant::now());
    filter_apps(app);
    let c = &app.cfg.launcher;
    let rows = c.rows.clamp(3, 12);
    let w = c.width.clamp(360, 1000) + 2 * SHADOW as u32;
    let h = 70 + rows * 56 + 22 + 2 * SHADOW as u32;
    let (anchor, margin) = match c.position.as_str() {
        "top" => (Anchor::TOP, (90, 0, 0, 0)),
        "bottom" => (Anchor::BOTTOM, (0, 0, crate::shell::bar_height(app) as i32 + 6, 0)),
        _ => (Anchor::empty(), (0, 0, 0, 0)),
    };
    app.open(Spec {
        kind: Kind::Launcher,
        layer: Layer::Overlay,
        ns: "dshell-launcher",
        anchor,
        size: (w, h),
        exclusive: -1,
        margin,
        kb: KeyboardInteractivity::Exclusive,
        output: app.svc.main_output.clone(),
        topics: topic::APPS | topic::THEME,
        fps: 0,
    });
}

pub fn close_launcher(app: &mut App) {
    if app.has(Kind::Launcher) {
        app.ui.launcher.rev.set(false, Instant::now());
        app.mark_kind(Kind::Launcher);
    }
}

pub fn filter_apps(app: &mut App) {
    let q = app.ui.launcher.query.trim().to_lowercase();
    if app.ui.launcher.clip_mode {
        let l = &mut app.ui.launcher;
        l.results = l.clip.iter().enumerate().filter(|(_, c)| q.is_empty() || c.1.to_lowercase().contains(&q)).map(|(i, _)| i).collect();
        l.sel = 0;
        l.top = 0;
        app.mark_kind(Kind::Launcher);
        return;
    }
    let usage = &app.saved.app_usage;
    let mut v: Vec<(i32, u32, usize)> = app
        .svc
        .apps
        .list
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let s = crate::svc::apps::score(e, &q);
            (s > 0).then(|| (s, usage.get(&e.id).copied().unwrap_or(0), i))
        })
        .collect();
    if q.is_empty() {
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
    } else {
        v.sort_by(|a, b| (b.0 + b.1.min(50) as i32 * 4).cmp(&(a.0 + a.1.min(50) as i32 * 4)).then(a.2.cmp(&b.2)));
    }
    app.ui.launcher.results = v.into_iter().map(|x| x.2).collect();
    app.ui.launcher.sel = 0;
    app.ui.launcher.top = 0;
    app.mark_kind(Kind::Launcher);
}

pub fn toggle_control(app: &mut App) {
    if app.has(Kind::Control) {
        app.ui.control.rev.set(false, Instant::now());
        app.mark_kind(Kind::Control);
        crate::svc::sysmon::update_wanted(app);
        return;
    }
    close_others(app, Kind::Control);
    app.ui.control.rev.set(true, Instant::now());
    app.ui.control.view = 0;
    app.ui.control.pw_for = None;
    app.ui.control.pw.clear();
    let w = app.cfg.control.width.clamp(360, 600) + 2 * SHADOW as u32;
    let top = bar_top(app);
    app.open(Spec {
        kind: Kind::Control,
        layer: Layer::Top,
        ns: "dshell-control",
        anchor: Anchor::TOP | Anchor::RIGHT | Anchor::BOTTOM,
        size: (w, 0),
        exclusive: 0,
        margin: if top { (0, 0, 0, 0) } else { (0, 0, 0, 0) },
        kb: KeyboardInteractivity::OnDemand,
        output: app.svc.main_output.clone(),
        topics: topic::AUDIO
            | topic::MEDIA
            | topic::NET
            | topic::BT
            | topic::NOTIF
            | topic::TOGGLES
            | topic::THEME
            | topic::CLOCK
            | topic::SYS
            | topic::WEATHER
            | topic::BATT,
        fps: 0,
    });
    brightness_read(app);
    crate::svc::sysmon::update_wanted(app);
}

pub fn toggle_power(app: &mut App) {
    if app.has(Kind::Power) {
        app.ui.power.set(false, Instant::now());
        app.mark_kind(Kind::Power);
        return;
    }
    close_others(app, Kind::Power);
    app.ui.power.set(true, Instant::now());
    app.ui.power_sel = 0;
    app.open(Spec {
        kind: Kind::Power,
        layer: Layer::Overlay,
        ns: "dshell-power",
        anchor: Anchor::empty(),
        size: (600 + 2 * SHADOW as u32, 200 + 2 * SHADOW as u32),
        exclusive: -1,
        margin: (0, 0, 0, 0),
        kb: KeyboardInteractivity::Exclusive,
        output: app.svc.main_output.clone(),
        topics: topic::THEME,
        fps: 0,
    });
}

pub fn toggle_picker(app: &mut App) {
    if app.has(Kind::Picker) {
        app.ui.picker.rev.set(false, Instant::now());
        app.mark_kind(Kind::Picker);
        return;
    }
    close_others(app, Kind::Picker);
    let list = crate::shell::wallpaper_list(app);
    let cur = app.st.wall.path.clone();
    let p = &mut app.ui.picker;
    p.sel = list.iter().position(|x| x.to_string_lossy() == cur).unwrap_or(0);
    p.list = list;
    p.thumbs.clear();
    p.rev.set(true, Instant::now());
    let top = bar_top(app);
    let bh = crate::shell::bar_height(app) as i32;
    app.open(Spec {
        kind: Kind::Picker,
        layer: Layer::Overlay,
        ns: "dshell-picker",
        anchor: if top { Anchor::TOP } else { Anchor::BOTTOM },
        size: (980 + 2 * SHADOW as u32, 300 + 2 * SHADOW as u32),
        exclusive: -1,
        margin: if top { (bh, 0, 0, 0) } else { (0, 0, bh, 0) },
        kb: KeyboardInteractivity::Exclusive,
        output: app.svc.main_output.clone(),
        topics: topic::THEME,
        fps: 0,
    });
}

pub fn toggle_settings(app: &mut App) {
    if app.has(Kind::Settings) {
        app.ui.settings.rev.set(false, Instant::now());
        app.mark_kind(Kind::Settings);
        return;
    }
    close_others(app, Kind::Settings);
    app.ui.settings.rev.set(true, Instant::now());
    app.ui.settings.scroll = 0.0;
    let (sw, sh) = app
        .svc
        .main_output
        .as_ref()
        .and_then(|o| app.outputs.info(o))
        .and_then(|i| i.logical_size)
        .map(|(w, h)| (w as u32, h as u32))
        .unwrap_or((1920, 1080));
    let w = 1080.min(sw.saturating_sub(60)).max(640);
    let h = 740.min(sh.saturating_sub(90)).max(460);
    app.open(Spec {
        kind: Kind::Settings,
        layer: Layer::Top,
        ns: "dshell-settings",
        anchor: Anchor::empty(),
        size: (w + 2 * SHADOW as u32, h + 2 * SHADOW as u32),
        exclusive: -1,
        margin: (0, 0, 0, 0),
        kb: KeyboardInteractivity::OnDemand,
        output: app.svc.main_output.clone(),
        topics: topic::THEME | topic::USER | topic::WM | topic::TOGGLES,
        fps: 0,
    });
}

/// A dock icon: bring the app's window forward when it is open, else start it.
fn dock_click(app: &mut App, key: &str) {
    let class = if let Some(c) = key.strip_prefix("class:") {
        Some(c.to_string())
    } else {
        let idx = app.svc.apps.list.iter().position(|e| e.id == key);
        app.st.wm.apps.iter().find(|(c, _, _)| crate::svc::apps::for_window(&app.svc.apps.list, c).map(|x| Some(x.0) == idx).unwrap_or(false)).map(|(c, _, _)| c.clone())
    };
    match class {
        Some(c) => match app.st.wm.kind {
            WmKind::Hyprland => hypr::dispatch(app, &format!("focuswindow class:^({})$", c.replace(['(', ')', '^', '$'], ""))),
            WmKind::Drift => {
                let _ = drift::request(&serde_json::json!({ "Focus": c }));
            }
            WmKind::Veyron => crate::svc::veyron::focus_app(app, &c),
            WmKind::None => {}
        },
        None => {
            if let Some(i) = app.svc.apps.list.iter().position(|e| e.id == key) {
                launch(app, i);
            }
        }
    }
}

/// A tray app's menu above (or below) its icon.
pub fn open_tray_menu(app: &mut App, service: String, path: String, entries: Vec<crate::svc::tray::MenuEntry>, x: i32) {
    close_others(app, Kind::Menu);
    app.destroy(Kind::Menu);
    let h = crate::ui::menu::height(&entries) as u32;
    let w = 280u32;
    app.ui.menu = crate::ui::MenuUi { service, path, entries, rev: crate::anim::Reveal::new(200, 140) };
    app.ui.menu.rev.set(true, Instant::now());
    let top = bar_top(app);
    let bh = if app.cfg.bar.enabled { crate::shell::bar_height(app) as i32 - 8 } else { 8 };
    let sw = app.svc.main_output.as_ref().and_then(|o| app.outputs.info(o)).and_then(|i| i.logical_size).map(|s| s.0).unwrap_or(1920);
    let left = (x - w as i32 / 2 - SHADOW).clamp(0, (sw - w as i32 - 2 * SHADOW).max(0));
    app.open(Spec {
        kind: Kind::Menu,
        layer: Layer::Top,
        ns: "dshell-menu",
        anchor: if top { Anchor::TOP } else { Anchor::BOTTOM } | Anchor::LEFT,
        size: (w + 2 * SHADOW as u32, h + 2 * SHADOW as u32),
        exclusive: -1,
        margin: if top { (bh, 0, 0, left) } else { (0, 0, bh, left) },
        kb: KeyboardInteractivity::OnDemand,
        output: app.svc.main_output.clone(),
        topics: topic::THEME,
        fps: 0,
    });
}

fn close_menu(app: &mut App) {
    if app.has(Kind::Menu) {
        app.ui.menu.rev.set(false, Instant::now());
        app.mark_kind(Kind::Menu);
    }
}

fn close_others(app: &mut App, keep: Kind) {
    let now = Instant::now();
    if keep != Kind::Menu && app.has(Kind::Menu) {
        app.ui.menu.rev.set(false, now);
        app.mark_kind(Kind::Menu);
    }
    if keep != Kind::Settings && app.has(Kind::Settings) {
        app.ui.settings.rev.set(false, now);
        app.mark_kind(Kind::Settings);
    }
    if keep != Kind::Launcher && app.has(Kind::Launcher) {
        app.ui.launcher.rev.set(false, now);
        app.mark_kind(Kind::Launcher);
    }
    if keep != Kind::Control && app.has(Kind::Control) {
        app.ui.control.rev.set(false, now);
        app.mark_kind(Kind::Control);
    }
    if keep != Kind::Power && app.has(Kind::Power) {
        app.ui.power.set(false, now);
        app.mark_kind(Kind::Power);
    }
    if keep != Kind::Picker && app.has(Kind::Picker) {
        app.ui.picker.rev.set(false, now);
        app.mark_kind(Kind::Picker);
    }
    if keep != Kind::Card && app.ui.card.pinned {
        close_card(app);
    }
}

pub fn close_all(app: &mut App) {
    close_others(app, Kind::Bar);
    close_card(app);
}

// ------------------------------------------------------------------ the island

pub fn island_flash(app: &mut App, mode: IslandMode) {
    if !app.cfg.bar.enabled || !app.cfg.island.dynamic {
        return;
    }
    let allowed = match mode {
        IslandMode::Volume | IslandMode::Mic | IslandMode::Bright => app.cfg.island.volume,
        IslandMode::Ws => app.cfg.island.workspaces,
        IslandMode::Charger => app.cfg.island.charger,
        IslandMode::Idle => true,
    };
    if !allowed {
        return;
    }
    let now = Instant::now();
    let isl = &mut app.ui.island;
    if isl.mode != mode {
        isl.face.jump(0.0);
        isl.face.set(1.0, now);
    }
    isl.mode = mode;
    isl.mode_at = now;
    isl.pulse = Some(now);
    app.mark_kind(Kind::Bar);
    let stay = if mode == IslandMode::Charger { 2600 } else { 1500 };
    let _ = app.lh.insert_source(Timer::from_duration(Duration::from_millis(stay)), move |_, _, app: &mut App| {
        let isl = &mut app.ui.island;
        if isl.mode == mode && isl.mode_at.elapsed() >= Duration::from_millis(stay - 20) {
            isl.mode = IslandMode::Idle;
            isl.face.jump(0.0);
            isl.face.set(1.0, Instant::now());
            app.mark_kind(Kind::Bar);
        }
        TimeoutAction::Drop
    });
}

// ------------------------------------------------------------------ notifications

pub fn notif_arrived(app: &mut App, id: u32) {
    app.mark(topic::NOTIF);
    if app.st.toggles.dnd {
        return;
    }
    let (urgency, timeout) = app
        .st
        .notifs
        .list
        .iter()
        .find(|n| n.id == id)
        .map(|n| (n.urgency, n.timeout_ms))
        .unwrap_or((1, -1));
    app.st.notifs.popup = Some(id);
    app.st.notifs.popup_at = Some(Instant::now());
    if app.ui.card.pinned && app.has(Kind::Card) {
        return; // a card you opened stays; the notification is in the list
    }
    app.ui.card.notif = Some(id);
    open_card(app, CardMode::Notif, false);
    if urgency >= 2 {
        return; // critical: stays until dismissed
    }
    let ms = if timeout > 0 { timeout.clamp(2000, 30000) as u64 } else { app.cfg.notifications.timeout.clamp(2, 60) as u64 * 1000 };
    let _ = app.lh.insert_source(Timer::from_duration(Duration::from_millis(ms)), move |_, _, app: &mut App| {
        expire_popup(app, id);
        TimeoutAction::Drop
    });
}

fn expire_popup(app: &mut App, id: u32) {
    if app.st.notifs.popup != Some(id) {
        return;
    }
    // pointing at it holds it
    if app.ui.card.hovered {
        let _ = app.lh.insert_source(Timer::from_duration(Duration::from_millis(1500)), move |_, _, app: &mut App| {
            expire_popup(app, id);
            TimeoutAction::Drop
        });
        return;
    }
    app.st.notifs.popup = None;
    if app.ui.card.mode == CardMode::Notif && !app.ui.card.pinned {
        close_card(app);
    }
    app.mark(topic::NOTIF);
}

/// A message from the shell itself (config errors, the focus timer).
pub fn system_notice(app: &mut App, summary: &str, body: &str) {
    let n = &mut app.st.notifs;
    n.next_id = n.next_id.wrapping_add(1).max(1);
    let id = n.next_id;
    n.list.insert(
        0,
        crate::state::Notif {
            id,
            app: "DarkShadow".into(),
            summary: summary.into(),
            body: body.into(),
            icon: String::new(),
            image: None,
            actions: Vec::new(),
            urgency: 1,
            timeout_ms: -1,
            at: Instant::now(),
            wall: std::time::SystemTime::now(),
        },
    );
    notif_arrived(app, id);
}

// ------------------------------------------------------------------ hardware

pub fn brightness_read(app: &mut App) {
    let Ok(rd) = std::fs::read_dir("/sys/class/backlight") else { return };
    let Some(dev) = rd.flatten().next() else { return };
    let p = dev.path();
    let max = std::fs::read_to_string(p.join("max_brightness")).ok().and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(0);
    let cur = std::fs::read_to_string(p.join("brightness")).ok().and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(0);
    if max > 0 {
        let b = &mut app.st.bright;
        b.ok = true;
        b.max = max;
        b.pct = cur as f32 / max as f32;
        b.device = dev.file_name().to_string_lossy().into_owned();
        app.mark(topic::AUDIO);
    }
}

pub fn brightness_set(app: &mut App, v: f32) {
    if !app.st.bright.ok {
        brightness_read(app);
        if !app.st.bright.ok {
            return;
        }
    }
    let v = v.clamp(0.01, 1.0);
    app.st.bright.pct = v;
    let raw = (v * app.st.bright.max as f32).round() as u32;
    let dev = app.st.bright.device.clone();
    if !bus::set_backlight(app, &dev, raw) && crate::proc::which("brightnessctl") {
        crate::proc::spawn(&["brightnessctl", "-q", "set", &format!("{}%", (v * 100.0) as u32)]);
    }
    app.mark(topic::AUDIO);
}

fn kill_child(c: &mut Option<std::process::Child>) {
    if let Some(mut ch) = c.take() {
        let _ = ch.kill();
        let _ = ch.wait();
    }
}

fn spawn_child(argv: &[&str]) -> Option<std::process::Child> {
    std::process::Command::new(argv[0])
        .args(&argv[1..])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()
}

fn set_nightlight(app: &mut App, on: bool) {
    let c = app.cfg.control.clone();
    if on {
        if !c.nightlight_on.is_empty() {
            crate::proc::sh(&c.nightlight_on);
        } else if app.st.wm.kind == WmKind::Hyprland && crate::proc::which("hyprsunset") {
            app.svc.nightlight = spawn_child(&["hyprsunset", "-t", "4000"]);
        } else if crate::proc::which("wlsunset") {
            app.svc.nightlight = spawn_child(&["wlsunset", "-T", "4001", "-t", "4000"]);
        } else if crate::proc::which("gammastep") {
            app.svc.nightlight = spawn_child(&["gammastep", "-O", "4000", "-m", "wayland"]);
        }
    } else {
        kill_child(&mut app.svc.nightlight);
        if !c.nightlight_off.is_empty() {
            crate::proc::sh(&c.nightlight_off);
        }
    }
    app.st.toggles.nightlight = on;
}

fn set_awake(app: &mut App, on: bool) {
    if on {
        app.svc.awake = spawn_child(&[
            "systemd-inhibit",
            "--what=idle:sleep",
            "--who=dshell",
            "--why=Keep awake",
            "sleep",
            "infinity",
        ]);
        app.st.toggles.awake = app.svc.awake.is_some();
    } else {
        kill_child(&mut app.svc.awake);
        app.st.toggles.awake = false;
    }
}

fn set_gaming(app: &mut App, on: bool) {
    app.st.toggles.gaming = on;
    app.theme = make_theme(&app.cfg, &app.st);
    crate::svc::viz::update_wanted(app);
    app.mark(topic::ALL);
}

pub fn power(app: &mut App, p: PowerAct) {
    let lock = app.cfg.control.lock.clone();
    match p {
        PowerAct::Lock => {
            if !lock.is_empty() {
                crate::proc::sh(&lock);
            } else if crate::proc::which("hyprlock") {
                crate::proc::spawn(&["hyprlock"]);
            } else if crate::proc::which("swaylock") {
                crate::proc::spawn(&["swaylock", "-f"]);
            } else {
                crate::proc::spawn(&["loginctl", "lock-session"]);
            }
        }
        PowerAct::Suspend => crate::proc::spawn(&["systemctl", "suspend"]),
        PowerAct::Logout => match app.st.wm.kind {
            WmKind::Hyprland => hypr::dispatch(app, "exit"),
            WmKind::Drift => drift::action("quit"),
            WmKind::Veyron => {
                crate::svc::veyron::action("quit");
            }
            WmKind::None => crate::proc::spawn(&["loginctl", "terminate-session", ""]),
        },
        PowerAct::Reboot => crate::proc::spawn(&["systemctl", "reboot"]),
        PowerAct::Shutdown => crate::proc::spawn(&["systemctl", "poweroff"]),
    }
    close_all(app);
}

// ------------------------------------------------------------------ the focus timer

pub fn focus_tick(app: &mut App) {
    let now = Instant::now();
    let f = &mut app.ui.focus;
    if f.running && f.remaining(now) <= 0.0 {
        f.running = false;
        f.end = None;
        let was_break = f.on_break;
        if !was_break {
            f.done_sessions += 1;
        }
        f.on_break = !was_break;
        f.left = f.total();
        let (s, b) = if was_break { ("Back to focus", "Your break is over.") } else { ("Time for a break", "Nice work. Stand up for a moment.") };
        system_notice(app, s, b);
    }
}

fn focus_toggle(app: &mut App) {
    let now = Instant::now();
    let f = &mut app.ui.focus;
    if f.running {
        f.left = f.remaining(now);
        f.running = false;
        f.end = None;
    } else {
        f.end = Some(now + Duration::from_secs_f64(f.left.max(1.0)));
        f.running = true;
    }
    crate::shell::restart_clock(app);
    app.mark(topic::SECOND);
}

// ------------------------------------------------------------------ clicks

pub fn act(app: &mut App, a: Act) {
    match a {
        Act::None => {}
        Act::OpenLauncher => open_launcher(app),
        Act::ToggleControl => toggle_control(app),
        Act::TogglePower => toggle_power(app),
        Act::OpenCard(m) => open_card(app, m, true),
        Act::CloseAll => close_all(app),
        Act::Ws(n) => match app.st.wm.kind {
            WmKind::Hyprland => hypr::dispatch(app, &format!("workspace {n}")),
            WmKind::Drift => drift::goto(app, n),
            WmKind::Veyron => crate::svc::veyron::goto(n),
            WmKind::None => {}
        },
        Act::WsRel(d) => match app.st.wm.kind {
            WmKind::Hyprland => hypr::dispatch(app, &format!("workspace {}", if d > 0 { "e+1" } else { "e-1" })),
            WmKind::Drift => drift::step(app, d),
            WmKind::Veyron => crate::svc::veyron::step(d),
            WmKind::None => {}
        },
        Act::PlayPause => bus::player_call(app, "PlayPause"),
        Act::Next => bus::player_call(app, "Next"),
        Act::Prev => bus::player_call(app, "Previous"),
        Act::SeekRel(s) => bus::seek_rel(app, s),
        Act::Mute => audio::toggle_mute(app, false),
        Act::MicMute => audio::toggle_mute(app, true),
        Act::Wifi => {
            let on = !app.st.net.wifi_on;
            bus::set_wifi(app, on);
        }
        Act::Bluetooth => {
            let on = !app.st.bt.powered;
            bus::set_bt(app, on);
        }
        Act::Dnd => {
            app.st.toggles.dnd = !app.st.toggles.dnd;
            app.saved.dnd = app.st.toggles.dnd;
            crate::config::save_saved(&app.saved);
            app.mark(topic::TOGGLES | topic::NOTIF);
        }
        Act::NightLight => {
            let on = !app.st.toggles.nightlight;
            set_nightlight(app, on);
            app.mark(topic::TOGGLES);
        }
        Act::Awake => {
            let on = !app.st.toggles.awake;
            set_awake(app, on);
            app.mark(topic::TOGGLES);
        }
        Act::Airplane => {
            let on = !app.st.toggles.airplane;
            app.st.toggles.airplane = on;
            if crate::proc::which("rfkill") {
                crate::proc::spawn(&["rfkill", if on { "block" } else { "unblock" }, "all"]);
            }
            if on {
                bus::set_wifi(app, false);
            }
            app.mark(topic::TOGGLES | topic::NET | topic::BT);
        }
        Act::Gaming => {
            let on = !app.st.toggles.gaming;
            set_gaming(app, on);
        }
        Act::NotifDismiss(id) => bus::close(app, id, 2),
        Act::NotifAction(id, key) => bus::invoke(app, id, &key),
        Act::NotifOpen(id) => {
            let has_default = app.st.notifs.list.iter().find(|n| n.id == id).map(|n| n.actions.iter().any(|a| a.0 == "default"));
            if has_default == Some(true) {
                bus::invoke(app, id, "default");
            } else {
                bus::close(app, id, 2);
            }
            if app.ui.card.mode == CardMode::Notif {
                close_card(app);
            }
        }
        Act::NotifClear => {
            let ids: Vec<u32> = app.st.notifs.list.iter().map(|n| n.id).collect();
            for id in ids {
                bus::close(app, id, 2);
            }
        }
        Act::Launch(i) => launch(app, i),
        Act::Power(p) => power(app, p),
        Act::FocusToggle => focus_toggle(app),
        Act::FocusReset => {
            let f = &mut app.ui.focus;
            f.running = false;
            f.end = None;
            f.left = f.total();
            app.mark(topic::SECOND);
        }
        Act::FocusSkip => {
            let f = &mut app.ui.focus;
            f.running = false;
            f.end = None;
            f.on_break = !f.on_break;
            f.left = f.total();
            app.mark(topic::SECOND);
        }
        Act::FocusMode(brk) => {
            let f = &mut app.ui.focus;
            if f.on_break != brk {
                f.on_break = brk;
                f.running = false;
                f.end = None;
                f.left = f.total();
            }
            app.mark(topic::SECOND);
        }
        Act::FocusMinutes(d) => {
            let f = &mut app.ui.focus;
            if f.on_break {
                f.break_minutes = (f.break_minutes as i32 + d).clamp(1, 60) as u32;
            } else {
                f.minutes = (f.minutes as i32 + d * 5).clamp(5, 120) as u32;
                app.saved.focus_minutes = f.minutes;
            }
            if !f.running {
                f.left = f.total();
            }
            crate::config::save_saved(&app.saved);
            app.mark(topic::SECOND);
        }
        Act::CardTab(t) => {
            app.ui.card.tab = t;
            app.ui.card.mode = if t == 1 { CardMode::System } else { CardMode::Today };
            crate::svc::sysmon::update_wanted(app);
            app.mark_kind(Kind::Card);
        }
        Act::ControlTab(t) => {
            app.ui.control.tab = t;
            crate::svc::sysmon::update_wanted(app);
            app.mark_kind(Kind::Control);
        }
        Act::Wallpaper(i) => {
            if let Some(p) = app.ui.picker.list.get(i).cloned() {
                app.ui.picker.sel = i;
                crate::shell::set_wallpaper(app, p, true);
                app.mark_kind(Kind::Picker);
            }
        }
        Act::WallShuffle => wallpaper_random(app),
        Act::OpenPicker => toggle_picker(app),
        Act::Settings => toggle_settings(app),
        Act::OpenConfigFile => open_settings(app),
        Act::Reload => crate::shell::reload(app),
        Act::NotifTest => system_notice(app, "Hello from DarkShadow", "Notifications are working. This one goes away by itself."),
        Act::EditMode => {
            app.ui.edit = !app.ui.edit;
            close_all(app);
            app.mark(topic::ALL);
        }
        Act::SettingsPage(i) => {
            app.ui.settings.page = i;
            app.ui.settings.scroll = 0.0;
            app.mark_kind(Kind::Settings);
        }
        Act::CfgToggle(k) => crate::settings::toggle(app, k),
        Act::CfgSet(k, v) => crate::settings::set(app, k, toml::Value::String(v)),
        Act::CfgSlide(_) => {}
        Act::RecStop => crate::svc::capture::stop(app),
        Act::TrayClick(k) => crate::svc::tray::click(app, &k, 1, 0, 0),
        Act::TrayMenu(k) => {
            let x = app.ui.last_press_x as i32;
            crate::svc::tray::click(app, &k, 2, x, 0)
        }
        Act::DockClick(key) => dock_click(app, &key),
        Act::DockPin(id) => {
            let mut pinned = app.cfg.dock.pinned.clone();
            if let Some(i) = pinned.iter().position(|p| p == &id) {
                pinned.remove(i);
            } else {
                pinned.push(id);
            }
            let v = toml::Value::Array(pinned.into_iter().map(toml::Value::String).collect());
            crate::settings::set(app, "dock.pinned", v);
        }
        Act::MenuPick(id) => {
            let (s, p) = (app.ui.menu.service.clone(), app.ui.menu.path.clone());
            crate::svc::tray::menu_event(app, &s, &p, id);
            close_menu(app);
        }
        Act::TrayAlt(k) => crate::svc::tray::click(app, &k, 3, 0, 0),
        Act::CcView(v) => {
            let was = app.ui.control.view;
            app.ui.control.view = v;
            app.ui.control.pw_for = None;
            app.ui.control.pw.clear();
            if v == 1 {
                bus::wifi_scan(app);
            }
            if v == 2 && app.st.bt.powered {
                bt_scan_for_a_while(app);
            }
            if was == 2 && v != 2 && app.st.bt.scanning {
                bus::bt_scan(app, false);
            }
            app.mark_kind(Kind::Control);
        }
        Act::WifiScan => bus::wifi_scan(app),
        Act::WifiPick(ssid) => {
            let ap = app.st.net.aps.iter().find(|a| a.ssid == ssid).cloned();
            if let Some(ap) = ap {
                if ap.active {
                    bus::wifi_disconnect(app);
                } else if ap.saved || !ap.secured {
                    app.ui.control.pw_for = None;
                    app.ui.control.pw.clear();
                    bus::wifi_connect(app, &ssid, None);
                } else {
                    app.ui.control.pw_for = Some(ssid);
                    app.ui.control.pw.clear();
                }
            }
            app.mark_kind(Kind::Control);
        }
        Act::PwSubmit => {
            if let Some(ssid) = app.ui.control.pw_for.take() {
                let pw = std::mem::take(&mut app.ui.control.pw);
                bus::wifi_connect(app, &ssid, Some(&pw));
            }
            app.mark_kind(Kind::Control);
        }
        Act::PwCancel => {
            app.ui.control.pw_for = None;
            app.ui.control.pw.clear();
            app.mark_kind(Kind::Control);
        }
        Act::BtScan => {
            if app.st.bt.scanning {
                bus::bt_scan(app, false);
            } else {
                bt_scan_for_a_while(app);
            }
        }
        Act::BtPick(path) => bus::bt_device(app, &path),
        Act::Run(cmd) => {
            crate::proc::sh(&cmd);
            close_all(app);
        }
        // sliders act while dragging
        Act::Seek | Act::Volume | Act::Mic | Act::Bright => {}
    }
}

/// Look for Bluetooth devices for 20 seconds.
fn bt_scan_for_a_while(app: &mut App) {
    bus::bt_scan(app, true);
    let _ = app.lh.insert_source(Timer::from_duration(Duration::from_secs(20)), |_, _, app: &mut App| {
        if app.st.bt.scanning {
            bus::bt_scan(app, false);
        }
        TimeoutAction::Drop
    });
}

fn open_settings(app: &mut App) {
    let cmd = app.cfg.control.settings.clone();
    close_all(app);
    if !cmd.is_empty() {
        crate::proc::sh(&cmd);
        return;
    }
    let p = crate::config::path();
    if !p.exists() {
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let _ = std::fs::write(&p, crate::config::DEFAULT_TOML);
    }
    crate::proc::spawn(&["xdg-open", &p.to_string_lossy()]);
}

fn launch(app: &mut App, i: usize) {
    if app.ui.launcher.clip_mode {
        // put that entry back on the clipboard
        if let Some((line, _)) = app.ui.launcher.clip.get(i).cloned() {
            let q = line.replace('\'', "'\\''");
            crate::proc::sh(&format!("printf '%s' '{q}' | cliphist decode | wl-copy"));
        }
        close_launcher(app);
        return;
    }
    let Some(e) = app.svc.apps.list.get(i).cloned() else { return };
    let term = app.cfg.apps.terminal.clone();
    let cmd = crate::svc::apps::command(&e, &term);
    *app.saved.app_usage.entry(e.id.clone()).or_insert(0) += 1;
    crate::config::save_saved(&app.saved);
    crate::proc::sh(&cmd);
    close_launcher(app);
}

fn wallpaper_random(app: &mut App) {
    let list = crate::shell::wallpaper_list(app);
    if list.is_empty() {
        return;
    }
    let cur = app.st.wall.path.clone();
    let seed = crate::clock::unix() as usize ^ (Instant::now().elapsed().subsec_nanos() as usize);
    let mut i = seed % list.len();
    if list.len() > 1 && list[i].to_string_lossy() == cur {
        i = (i + 1) % list.len();
    }
    crate::shell::set_wallpaper(app, list[i].clone(), true);
}

fn wallpaper_next(app: &mut App) {
    let list = crate::shell::wallpaper_list(app);
    if list.is_empty() {
        return;
    }
    let cur = app.st.wall.path.clone();
    let i = list.iter().position(|p| p.to_string_lossy() == cur).map(|i| (i + 1) % list.len()).unwrap_or(0);
    crate::shell::set_wallpaper(app, list[i].clone(), true);
}

// ------------------------------------------------------------------ sliders and the wheel

pub fn slide(app: &mut App, a: &Act, f: f32) {
    match a {
        Act::Volume => audio::set(app, false, f),
        Act::Mic => audio::set(app, true, f),
        Act::Bright => brightness_set(app, f),
        Act::CfgSlide(k) => crate::settings::slide(app, k, f),
        Act::Seek => {
            app.ui.island.pulse = None;
            app.svc.seek_preview = Some(f);
            app.mark(topic::MEDIA);
        }
        _ => {}
    }
}

pub fn slide_end(app: &mut App, a: &Act) {
    if *a == Act::Seek
        && let Some(f) = app.svc.seek_preview.take()
    {
        bus::seek_to(app, f);
    }
}

pub fn scroll(app: &mut App, a: &Act, d: f32) {
    match a {
        Act::Volume => audio::step(app, -d * 0.05),
        Act::Bright => brightness_set(app, app.st.bright.pct - d * 0.05),
        Act::WsRel(_) => act(app, Act::WsRel(if d > 0.0 { 1 } else { -1 })),
        Act::FocusMinutes(_) => act(app, Act::FocusMinutes(if d > 0.0 { -1 } else { 1 })),
        _ => {}
    }
}

pub fn scroll_surface(app: &mut App, kind: Kind, d: f32) {
    match kind {
        Kind::Settings => {
            app.ui.settings.scroll = (app.ui.settings.scroll + d * 56.0).max(0.0);
            app.mark_kind(Kind::Settings);
        }
        Kind::Launcher => {
            let l = &mut app.ui.launcher;
            let rows = app.cfg.launcher.rows.clamp(3, 12) as usize;
            let n = l.results.len();
            if d > 0.0 {
                l.top = (l.top + 1).min(n.saturating_sub(rows));
            } else {
                l.top = l.top.saturating_sub(1);
            }
            l.sel = l.sel.clamp(l.top, (l.top + rows).saturating_sub(1).min(n.saturating_sub(1)));
            app.mark_kind(Kind::Launcher);
        }
        Kind::Control => {
            app.ui.control.scroll = (app.ui.control.scroll + d * 48.0).max(0.0);
            app.mark_kind(Kind::Control);
        }
        Kind::Picker => {
            let n = app.ui.picker.list.len();
            if n > 0 {
                let p = &mut app.ui.picker;
                p.sel = if d > 0.0 { (p.sel + 1).min(n - 1) } else { p.sel.saturating_sub(1) };
                app.mark_kind(Kind::Picker);
            }
        }
        _ => {}
    }
}

// ------------------------------------------------------------------ pointer on surfaces

pub fn on_enter(app: &mut App, kind: Kind, entered: bool) {
    match kind {
        Kind::Card => {
            app.ui.card.hovered = entered;
            if !entered && !app.ui.card.pinned && app.ui.card.mode == CardMode::Player {
                schedule_card_close(app, 700);
            }
        }
        Kind::Bar => {
            app.svc.ptr_on_bar = entered;
            if app.cfg.bar.autohide {
                if entered {
                    app.ui.bar_rev.set(true, Instant::now());
                    app.mark_kind(Kind::Bar);
                } else {
                    let _ = app.lh.insert_source(Timer::from_duration(Duration::from_millis(700)), |_, _, app: &mut App| {
                        let card_open = app.has(Kind::Card) || app.has(Kind::Control);
                        let on_bar = app.svc.ptr_on_bar;
                        if !card_open && !on_bar && !app.svc.island_hovered {
                            app.ui.bar_rev.set(false, Instant::now());
                            app.mark_kind(Kind::Bar);
                        }
                        TimeoutAction::Drop
                    });
                }
            }
            if !entered && !app.ui.card.pinned && app.ui.card.mode == CardMode::Player && app.has(Kind::Card) {
                schedule_card_close(app, 700);
            }
        }
        _ => {}
    }
}

/// The pointer moved onto (or off) a clickable thing.
pub fn hover(app: &mut App, kind: Kind, a: Option<Act>) {
    if kind != Kind::Bar {
        return;
    }
    let on = a == Some(Act::OpenCard(CardMode::Today));
    if on == app.svc.island_hovered {
        return;
    }
    app.svc.island_hovered = on;
    if on {
        let _ = app.lh.insert_source(Timer::from_duration(Duration::from_millis(260)), |_, _, app: &mut App| {
            if app.svc.island_hovered {
                island_hover(app);
            }
            TimeoutAction::Drop
        });
    } else if app.has(Kind::Card) && !app.ui.card.pinned && app.ui.card.mode == CardMode::Player {
        schedule_card_close(app, 700);
    }
}

/// Called by the bar when the pointer rests on the island: the player grows out of it.
pub fn island_hover(app: &mut App) {
    if app.cfg.island.media && app.st.media.active() && !app.has(Kind::Card) && !app.has(Kind::Control) {
        open_card(app, CardMode::Player, false);
    }
}

fn schedule_card_close(app: &mut App, ms: u64) {
    app.ui.card.left_at = Some(Instant::now());
    let _ = app.lh.insert_source(Timer::from_duration(Duration::from_millis(ms)), move |_, _, app: &mut App| {
        let c = &app.ui.card;
        if !c.pinned && !c.hovered && c.left_at.map(|t| t.elapsed() >= Duration::from_millis(ms - 20)).unwrap_or(false) {
            let on_island = app.svc.island_hovered;
            if !on_island && app.ui.card.mode == CardMode::Player {
                close_card(app);
            }
        }
        TimeoutAction::Drop
    });
}

pub fn press_empty(app: &mut App, kind: Kind, button: u32, x: f32, y: f32) {
    match kind {
        Kind::Wall(_) => {
            if button == 0x111 {
                toggle_picker(app);
            } else {
                close_all(app);
            }
        }
        Kind::Widget(w) if app.ui.edit => {
            let c = w.cfg(&app.cfg);
            app.ui.drag = Some((w, (x, y), (c.x, c.y)));
        }
        Kind::Widget(_) => close_all(app),
        Kind::Launcher | Kind::Power | Kind::Picker | Kind::Settings => {
            // a click outside the card (in the shadow margin) closes it
            if let Some(s) = app.surf(kind) {
                let m = SHADOW as f32;
                if x < m || y < m || x > s.w as f32 - m || y > s.h as f32 - m {
                    close_all(app);
                }
            }
        }
        _ => {}
    }
}

pub fn drag_motion(app: &mut App, kind: Kind, x: f32, y: f32) -> bool {
    let Some((w, (sx, sy), (cx, cy))) = app.ui.drag else { return false };
    if kind != Kind::Widget(w) {
        return false;
    }
    // the surface moves under the pointer, so accumulate in screen terms
    let anchor = w.cfg(&app.cfg).anchor.clone();
    let (dx, dy) = (x - sx, y - sy);
    let (fx, fy) = match anchor.as_str() {
        "tr" => (-1.0, 1.0),
        "bl" => (1.0, -1.0),
        "br" => (-1.0, -1.0),
        _ => (1.0, 1.0),
    };
    let wc = w.cfg_mut(&mut app.cfg);
    wc.x = (cx as f32 + dx * fx) as i32;
    wc.y = (cy as f32 + dy * fy) as i32;
    let (nx, ny) = (wc.x, wc.y);
    app.ui.drag = Some((w, (sx, sy), (nx, ny)));
    let _ = (x, y);
    crate::shell::move_widget(app, w);
    true
}

pub fn drag_end(app: &mut App, kind: Kind) -> bool {
    let Some((w, _, _)) = app.ui.drag.take() else { return false };
    if kind != Kind::Widget(w) {
        return false;
    }
    save_widget_pos(app, w);
    true
}

fn save_widget_pos(app: &mut App, w: WidgetId) {
    // write only this widget's position into the user's config (keeping the rest of the file)
    let p = crate::config::path();
    let text = std::fs::read_to_string(&p).unwrap_or_default();
    let mut doc: toml::Table = toml::from_str(&text).unwrap_or_default();
    let c = w.cfg(&app.cfg).clone();
    let widgets = doc.entry("widgets").or_insert(toml::Value::Table(Default::default()));
    if let toml::Value::Table(t) = widgets {
        let e = t.entry(w.name()).or_insert(toml::Value::Table(Default::default()));
        if let toml::Value::Table(wt) = e {
            wt.insert("x".into(), toml::Value::Integer(c.x as i64));
            wt.insert("y".into(), toml::Value::Integer(c.y as i64));
            wt.insert("anchor".into(), toml::Value::String(c.anchor.clone()));
            wt.insert("enabled".into(), toml::Value::Boolean(c.enabled));
        }
    }
    let _ = std::fs::create_dir_all(p.parent().unwrap());
    app.svc.reload_armed = true; // our own write: no reload
    let _ = std::fs::write(&p, toml::to_string(&doc).unwrap_or_default());
    let _ = app.lh.insert_source(Timer::from_duration(Duration::from_millis(400)), |_, _, app: &mut App| {
        app.svc.reload_armed = false;
        TimeoutAction::Drop
    });
}

pub fn focus_lost(app: &mut App, kind: Kind) {
    match kind {
        Kind::Menu => close_menu(app),
        Kind::Control => {
            if app.has(Kind::Control) {
                toggle_control(app);
            }
        }
        Kind::Card => {
            if app.ui.card.pinned {
                close_card(app);
            }
        }
        _ => {}
    }
}

// ------------------------------------------------------------------ keys

pub fn key(app: &mut App, ev: &KeyEvent) {
    let Some(k) = app.kb_focus else { return };
    let sym = ev.keysym;
    if k == Kind::Control && app.ui.control.pw_for.is_some() {
        match sym {
            Keysym::Escape => act(app, Act::PwCancel),
            Keysym::Return | Keysym::KP_Enter => act(app, Act::PwSubmit),
            Keysym::BackSpace => {
                app.ui.control.pw.pop();
                app.mark_kind(Kind::Control);
            }
            _ => {
                if let Some(t) = &ev.utf8
                    && !t.chars().any(|c| c.is_control())
                {
                    app.ui.control.pw.push_str(t);
                    app.mark_kind(Kind::Control);
                }
            }
        }
        return;
    }
    if sym == Keysym::Escape {
        match k {
            Kind::Launcher => close_launcher(app),
            Kind::Control => toggle_control(app),
            Kind::Power => toggle_power(app),
            Kind::Picker => toggle_picker(app),
            Kind::Card => close_card(app),
            Kind::Settings => toggle_settings(app),
            Kind::Menu => close_menu(app),
            _ => {}
        }
        return;
    }
    match k {
        Kind::Settings => {
            let n = crate::settings::PAGES.len() as u8;
            let s = &mut app.ui.settings;
            match sym {
                Keysym::Tab | Keysym::Right => {
                    s.page = (s.page + 1) % n;
                    s.scroll = 0.0;
                }
                Keysym::ISO_Left_Tab | Keysym::Left => {
                    s.page = (s.page + n - 1) % n;
                    s.scroll = 0.0;
                }
                Keysym::Down => s.scroll += 56.0,
                Keysym::Up => s.scroll = (s.scroll - 56.0).max(0.0),
                Keysym::Page_Down => s.scroll += 400.0,
                Keysym::Page_Up => s.scroll = (s.scroll - 400.0).max(0.0),
                Keysym::Home => s.scroll = 0.0,
                _ => {}
            }
            app.mark_kind(Kind::Settings);
        }
        Kind::Launcher => launcher_key(app, ev),
        Kind::Power => {
            let n = 5;
            match sym {
                Keysym::Left | Keysym::Up => app.ui.power_sel = (app.ui.power_sel + n - 1) % n,
                Keysym::Right | Keysym::Down | Keysym::Tab => app.ui.power_sel = (app.ui.power_sel + 1) % n,
                Keysym::Return | Keysym::KP_Enter | Keysym::space => {
                    let p = crate::ui::power::ACTS[app.ui.power_sel].0;
                    power(app, p);
                }
                _ => {}
            }
            app.mark_kind(Kind::Power);
        }
        Kind::Picker => {
            let n = app.ui.picker.list.len();
            if n == 0 {
                return;
            }
            match sym {
                Keysym::Left | Keysym::Up => app.ui.picker.sel = app.ui.picker.sel.saturating_sub(1),
                Keysym::Right | Keysym::Down | Keysym::Tab => app.ui.picker.sel = (app.ui.picker.sel + 1).min(n - 1),
                Keysym::Return | Keysym::KP_Enter => {
                    let i = app.ui.picker.sel;
                    act(app, Act::Wallpaper(i));
                }
                _ => {
                    if ev.utf8.as_deref() == Some("r") {
                        wallpaper_random(app);
                    }
                }
            }
            app.mark_kind(Kind::Picker);
        }
        _ => {}
    }
}

fn launcher_key(app: &mut App, ev: &KeyEvent) {
    let rows = app.cfg.launcher.rows.clamp(3, 12) as usize;
    let n = app.ui.launcher.results.len();
    let ctrl = app.mods.ctrl;
    let l = &mut app.ui.launcher;
    match ev.keysym {
        Keysym::Down | Keysym::Tab => {
            if n > 0 {
                l.sel = (l.sel + 1) % n;
            }
        }
        Keysym::Up | Keysym::ISO_Left_Tab => {
            if n > 0 {
                l.sel = (l.sel + n - 1) % n;
            }
        }
        Keysym::Page_Down => l.sel = (l.sel + rows).min(n.saturating_sub(1)),
        Keysym::Page_Up => l.sel = l.sel.saturating_sub(rows),
        Keysym::Return | Keysym::KP_Enter => {
            if let Some(&i) = l.results.get(l.sel) {
                launch(app, i);
            } else if !l.query.trim().is_empty() {
                // nothing matched: run what was typed
                let q = l.query.clone();
                crate::proc::sh(&q);
                close_launcher(app);
            }
            return;
        }
        Keysym::BackSpace => {
            if ctrl {
                l.query.clear();
            } else {
                l.query.pop();
            }
            filter_apps(app);
            return;
        }
        _ => {
            if ctrl && ev.utf8.as_deref().map(|s| s.eq_ignore_ascii_case("u")).unwrap_or(false) {
                l.query.clear();
                filter_apps(app);
                return;
            }
            if let Some(t) = &ev.utf8
                && !t.chars().any(|c| c.is_control())
                && !ctrl
            {
                l.query.push_str(t);
                filter_apps(app);
                return;
            }
        }
    }
    let l = &mut app.ui.launcher;
    if l.sel < l.top {
        l.top = l.sel;
    } else if l.sel >= l.top + rows {
        l.top = l.sel + 1 - rows;
    }
    app.mark_kind(Kind::Launcher);
}

// ------------------------------------------------------------------ the CLI

fn panel_kind(name: &str) -> Option<&'static str> {
    Some(match name {
        "launcher" | "apps" => "launcher",
        "control" | "cc" | "controlcenter" | "notifications" => "control",
        "power" | "session" => "power",
        "picker" | "wallpaper" | "wallpapers" => "picker",
        "clipboard" | "clip" => "clipboard",
        "settings" | "config" => "settings",
        "today" | "dashboard" | "calendar" => "today",
        "system" => "system",
        "player" | "media" => "player",
        _ => return None,
    })
}

fn is_open(app: &App, p: &str) -> bool {
    match p {
        "launcher" => app.has(Kind::Launcher),
        "control" => app.has(Kind::Control),
        "power" => app.has(Kind::Power),
        "picker" => app.has(Kind::Picker),
        "settings" => app.has(Kind::Settings),
        "clipboard" => app.has(Kind::Launcher) && app.ui.launcher.clip_mode,
        "today" => app.has(Kind::Card) && app.ui.card.mode == CardMode::Today,
        "system" => app.has(Kind::Card) && app.ui.card.mode == CardMode::System,
        "player" => app.has(Kind::Card) && app.ui.card.mode == CardMode::Player,
        _ => false,
    }
}

fn toggle_panel(app: &mut App, p: &str) {
    match p {
        "launcher" => open_launcher(app),
        "control" => toggle_control(app),
        "power" => toggle_power(app),
        "picker" => toggle_picker(app),
        "settings" => toggle_settings(app),
        "clipboard" => open_clipboard(app),
        "today" => open_card(app, CardMode::Today, true),
        "system" => open_card(app, CardMode::System, true),
        "player" => open_card(app, CardMode::Player, true),
        _ => {}
    }
}

fn pct_arg(s: Option<&&str>) -> Option<f32> {
    s.and_then(|v| v.trim_end_matches('%').parse::<f32>().ok()).map(|v| (v / 100.0).clamp(0.0, 1.0))
}

pub fn command(app: &mut App, line: &str) -> String {
    let a: Vec<&str> = line.split_whitespace().collect();
    let r = match a.as_slice() {
        ["toggle", p] => match panel_kind(p) {
            Some(k) => {
                toggle_panel(app, k);
                Ok(())
            }
            None => Err(format!("unknown panel {p}")),
        },
        ["open", p] => match panel_kind(p) {
            Some(k) => {
                if !is_open(app, k) {
                    toggle_panel(app, k);
                }
                Ok(())
            }
            None => Err(format!("unknown panel {p}")),
        },
        ["close", "all"] | ["close"] => {
            close_all(app);
            Ok(())
        }
        ["close", p] => match panel_kind(p) {
            Some(k) => {
                if is_open(app, k) {
                    toggle_panel(app, k);
                }
                Ok(())
            }
            None => Err(format!("unknown panel {p}")),
        },
        ["volume", "up"] => {
            audio::step(app, 0.05);
            island_flash(app, IslandMode::Volume);
            Ok(())
        }
        ["volume", "down"] => {
            audio::step(app, -0.05);
            island_flash(app, IslandMode::Volume);
            Ok(())
        }
        ["volume", "mute"] => {
            audio::toggle_mute(app, false);
            island_flash(app, IslandMode::Volume);
            Ok(())
        }
        ["volume", "set", v] => match pct_arg(Some(v)) {
            Some(v) => {
                audio::set(app, false, v);
                island_flash(app, IslandMode::Volume);
                Ok(())
            }
            None => Err("volume set <0-100>".into()),
        },
        ["mic", "mute"] => {
            audio::toggle_mute(app, true);
            island_flash(app, IslandMode::Mic);
            Ok(())
        }
        ["brightness", "up"] => {
            brightness_read(app);
            brightness_set(app, app.st.bright.pct + 0.05);
            island_flash(app, IslandMode::Bright);
            Ok(())
        }
        ["brightness", "down"] => {
            brightness_read(app);
            brightness_set(app, app.st.bright.pct - 0.05);
            island_flash(app, IslandMode::Bright);
            Ok(())
        }
        ["brightness", "set", v] => match pct_arg(Some(v)) {
            Some(v) => {
                brightness_set(app, v);
                island_flash(app, IslandMode::Bright);
                Ok(())
            }
            None => Err("brightness set <0-100>".into()),
        },
        ["media", "play-pause"] | ["media", "toggle"] => {
            act(app, Act::PlayPause);
            Ok(())
        }
        ["media", "next"] => {
            act(app, Act::Next);
            Ok(())
        }
        ["media", "prev"] | ["media", "previous"] => {
            act(app, Act::Prev);
            Ok(())
        }
        ["wallpaper", "set", rest @ ..] if !rest.is_empty() => {
            let p = crate::config::expand(&rest.join(" "));
            if p.is_file() {
                crate::shell::set_wallpaper(app, p, true);
                Ok(())
            } else {
                Err(format!("no such file: {}", p.display()))
            }
        }
        ["wallpaper", "next"] => {
            wallpaper_next(app);
            Ok(())
        }
        ["wallpaper", "random"] => {
            wallpaper_random(app);
            Ok(())
        }
        ["scheme", s] => {
            app.cfg.theme.scheme = s.to_string();
            app.theme = make_theme(&app.cfg, &app.st);
            app.mark(topic::ALL);
            Ok(())
        }
        ["notifs", "clear"] => {
            act(app, Act::NotifClear);
            Ok(())
        }
        ["notifs", "dnd"] => {
            act(app, Act::Dnd);
            Ok(())
        }
        ["notifs", "test"] => {
            system_notice(app, "Hello from DarkShadow", "Notifications are working. This one goes away by itself.");
            Ok(())
        }
        ["ws", "next"] => {
            act(app, Act::WsRel(1));
            Ok(())
        }
        ["ws", "prev"] => {
            act(app, Act::WsRel(-1));
            Ok(())
        }
        ["ws", n] => match n.parse::<i32>() {
            Ok(n) => {
                act(app, Act::Ws(n));
                Ok(())
            }
            Err(_) => Err("ws <n>|next|prev".into()),
        },
        ["edit"] => {
            app.ui.edit = !app.ui.edit;
            app.mark(topic::ALL);
            Ok(())
        }
        ["gaming"] => {
            act(app, Act::Gaming);
            Ok(())
        }
        ["focus", "toggle"] => {
            act(app, Act::FocusToggle);
            Ok(())
        }
        ["focus", "reset"] => {
            act(app, Act::FocusReset);
            Ok(())
        }
        ["focus", "skip"] => {
            act(app, Act::FocusSkip);
            Ok(())
        }
        ["shot"] | ["screenshot"] => {
            crate::svc::capture::shot(app, "region");
            Ok(())
        }
        ["shot", w] | ["screenshot", w] => {
            crate::svc::capture::shot(app, w);
            Ok(())
        }
        ["record"] => {
            crate::svc::capture::record(app, "full");
            Ok(())
        }
        ["record", w] => {
            crate::svc::capture::record(app, w);
            Ok(())
        }
        ["lock"] => {
            power(app, PowerAct::Lock);
            Ok(())
        }
        ["reload"] => {
            crate::shell::reload(app);
            Ok(())
        }
        ["quit"] | ["exit"] => {
            app.exit = true;
            Ok(())
        }
        ["status"] => return status(app),
        ["debug-click", p, x, y] | ["debug-click", p, x, y, _] => {
            let kind = match *p {
                "bar" => Kind::Bar,
                "card" => Kind::Card,
                "launcher" => Kind::Launcher,
                "control" => Kind::Control,
                "power" => Kind::Power,
                "picker" => Kind::Picker,
                "settings" => Kind::Settings,
                "menu" => Kind::Menu,
                "dock" => Kind::Dock,
                _ => return "error: panel".into(),
            };
            if a.get(4) == Some(&"right") {
                return app.debug_right(kind, x.parse().unwrap_or(0.0), y.parse().unwrap_or(0.0));
            }
            let wheel = a.get(4).and_then(|w| w.parse::<f32>().ok()).unwrap_or(0.0);
            return app.debug_click(kind, x.parse().unwrap_or(0.0), y.parse().unwrap_or(0.0), wheel);
        }
        _ => Err(format!("unknown command `{line}` (see dshell --help)")),
    };
    match r {
        Ok(()) => "ok".into(),
        Err(e) => format!("error: {e}"),
    }
}

fn status(app: &App) -> String {
    let rss = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmRSS")).map(|l| l.split_whitespace().nth(1).unwrap_or("0").to_string()))
        .unwrap_or_default();
    let surfs: Vec<String> =
        app.surfs.iter().map(|s| format!("{:?}[{}x{} draws={} avg={}us pool={}k]", s.kind, s.w, s.h, s.draws, s.draw_us / s.draws.max(1) as u64, s.pool_len / 1024)).collect();
    format!(
        "wm: {:?}\nworkspace: {} of {}\nworkspaces: {}\nwindow: {} ({}){}\napps: {}\nmedia: {} — {} ({})\nvolume: {:.0}%{}\nnotifications: {} (server: {})\nsurfaces: {}\nmemory: {} kB",
        app.st.wm.kind,
        app.st.wm.active,
        app.st.wm.workspaces.len(),
        app.st.wm.workspaces.iter().map(|w| format!("{}:{}{}", w.id, w.name, if w.windows > 0 { format!("[{}]", w.windows) } else { String::new() })).collect::<Vec<_>>().join(" "),
        app.st.wm.title,
        app.st.wm.class,
        if app.st.wm.covered { " covered" } else { "" },
        app.st.wm.apps.iter().map(|(c, n, f)| format!("{c}x{n}{}", if *f { "*" } else { "" })).collect::<Vec<_>>().join(" "),
        app.st.media.title,
        app.st.media.artist,
        if app.st.media.playing { "playing" } else { "paused" },
        app.st.audio.vol * 100.0,
        if app.st.audio.muted { " muted" } else { "" },
        app.st.notifs.list.len(),
        app.st.notifs.owned,
        surfs.join(", "),
        rss
    )
}
