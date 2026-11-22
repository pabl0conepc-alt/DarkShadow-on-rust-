//! What is on screen and when: the persistent surfaces (wallpaper, bar, widgets, visualizer), the clock tick,
//! the live config reload and the wallpaper.

use std::os::fd::{AsFd, BorrowedFd, RawFd};
use std::path::PathBuf;
use std::time::Duration;

use smithay_client_toolkit::{
    reexports::{
        calloop::{
            Interest, Mode, PostAction,
            generic::Generic,
            timer::{TimeoutAction, Timer},
        },
        client::Proxy,
    },
    shell::{
        WaylandSurface,
        wlr_layer::{Anchor, KeyboardInteractivity, Layer},
    },
};

use crate::app::{App, Spec, make_theme};
use crate::state::topic;
use crate::ui::{Kind, WIDGETS, WidgetId};

/// Room around a widget card for its shadow.
pub const SHADOW: i32 = 18;

pub fn widget_size(w: WidgetId, scale: f32) -> (u32, u32) {
    let (a, b) = match w {
        WidgetId::Clock => (190, 300),
        WidgetId::User => (340, 118),
        WidgetId::Trio => (340, 116),
        WidgetId::Focus => (258, 298),
        WidgetId::Player => (320, 380),
        WidgetId::Weather => (300, 150),
        WidgetId::Calendar => (300, 300),
    };
    (((a as f32 * scale) as i32 + SHADOW * 2) as u32, ((b as f32 * scale) as i32 + SHADOW * 2) as u32)
}

pub fn bar_height(app: &App) -> u32 {
    app.cfg.bar.height.clamp(28, 64) + 16
}

pub fn start(app: &mut App) {
    // saved state: wallpaper, do-not-disturb, focus length
    app.st.toggles.dnd = app.saved.dnd || app.cfg.notifications.dnd;
    if app.saved.focus_minutes > 0 {
        app.ui.focus.minutes = app.saved.focus_minutes;
        app.ui.focus.left = app.saved.focus_minutes as f64 * 60.0;
    }
    pick_output(app);
    crate::svc::start(app);
    build(app);
    clock_tick(app);
    watch_config(app);
    let wp = current_wallpaper(app);
    if let Some(p) = wp {
        set_wallpaper(app, p, false);
    }
    if let Some(e) = app.cfg_err.clone() {
        crate::actions::system_notice(app, "Config error", &e);
    }
}

fn pick_output(app: &mut App) {
    if app.svc.main_output.is_some() {
        return;
    }
    let want = app.cfg.bar.output.clone();
    let outs: Vec<_> = app.outputs.outputs().collect();
    let chosen = outs
        .iter()
        .find(|o| !want.is_empty() && app.outputs.info(o).and_then(|i| i.name).as_deref() == Some(want.as_str()))
        .or(outs.first())
        .cloned();
    app.svc.main_output = chosen;
}

pub fn outputs_changed(app: &mut App) {
    let had = app.svc.main_output.is_some();
    pick_output(app);
    if !had && app.svc.main_output.is_some() && app.started.elapsed() > Duration::from_millis(10) {
        build(app);
    }
    if app.cfg.wallpaper.enabled {
        let outs: Vec<_> = app.outputs.outputs().collect();
        for o in outs {
            let id = o.id().protocol_id();
            if !app.has(Kind::Wall(id)) {
                open_wall(app, o);
                if let Some(p) = current_wallpaper(app) {
                    set_wallpaper(app, p, false);
                }
            }
        }
    }
}

/// (Re)create the surfaces that are always there.
pub fn build(app: &mut App) {
    let keep_wall = app.cfg.wallpaper.enabled;
    let old: Vec<Kind> = app
        .surfs
        .iter()
        .map(|s| s.kind)
        .filter(|k| matches!(k, Kind::Bar | Kind::Widget(_) | Kind::Viz) || (!keep_wall && matches!(k, Kind::Wall(_))))
        .collect();
    for k in old {
        app.destroy(k);
    }
    let Some(out) = app.svc.main_output.clone() else { return };
    if app.cfg.wallpaper.enabled {
        let outs: Vec<_> = app.outputs.outputs().collect();
        for o in outs {
            if !app.has(Kind::Wall(o.id().protocol_id())) {
                open_wall(app, o);
            }
        }
    }
    let c = app.cfg.clone();
    if c.widgets.enabled {
        if c.widgets.visualizer.enabled {
            app.open(Spec {
                kind: Kind::Viz,
                layer: Layer::Bottom,
                ns: "dshell-viz",
                anchor: Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                size: (0, c.widgets.visualizer.height.clamp(30, 400)),
                exclusive: -1,
                margin: (0, 0, 0, 0),
                kb: KeyboardInteractivity::None,
                output: Some(out.clone()),
                topics: topic::VIZ | topic::THEME | topic::MEDIA,
                fps: c.widgets.visualizer.fps.clamp(10, 60),
            });
        }
        for w in WIDGETS {
            let wc = w.cfg(&c);
            if !wc.enabled {
                continue;
            }
            open_widget(app, w, out.clone());
        }
    }
    if c.bar.enabled {
        let top = c.bar.position == "top";
        let h = bar_height(app);
        app.open(Spec {
            kind: Kind::Bar,
            layer: Layer::Top,
            ns: "dshell-bar",
            anchor: if top { Anchor::TOP } else { Anchor::BOTTOM } | Anchor::LEFT | Anchor::RIGHT,
            size: (0, h),
            exclusive: if c.bar.autohide { 0 } else { h as i32 - 4 },
            margin: (0, 0, 0, 0),
            kb: KeyboardInteractivity::None,
            output: Some(out),
            topics: topic::WM
                | topic::AUDIO
                | topic::MEDIA
                | topic::NET
                | topic::BT
                | topic::BATT
                | topic::NOTIF
                | topic::CLOCK
                | topic::THEME
                | topic::WEATHER
                | topic::VIZ
                | topic::TOGGLES,
            fps: 0,
        });
    }
}

fn open_wall(app: &mut App, o: smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput) {
    app.open(Spec {
        kind: Kind::Wall(o.id().protocol_id()),
        layer: Layer::Background,
        ns: "dshell-wallpaper",
        anchor: Anchor::all(),
        size: (0, 0),
        exclusive: -1,
        margin: (0, 0, 0, 0),
        kb: KeyboardInteractivity::None,
        output: Some(o),
        topics: 0,
        fps: 30,
    });
}

pub fn widget_topics(w: WidgetId) -> u32 {
    topic::THEME
        | match w {
            WidgetId::Clock => topic::CLOCK,
            WidgetId::User => topic::CLOCK | topic::USER,
            WidgetId::Trio => topic::SYS | topic::WM,
            WidgetId::Focus => topic::SECOND | topic::WM,
            WidgetId::Player => topic::MEDIA | topic::WM,
            WidgetId::Weather => topic::WEATHER,
            WidgetId::Calendar => topic::CLOCK,
        }
}

pub fn widget_margin(app: &App, w: WidgetId) -> (Anchor, (i32, i32, i32, i32)) {
    let wc = w.cfg(&app.cfg);
    let (x, y) = (wc.x - SHADOW, wc.y - SHADOW);
    match wc.anchor.as_str() {
        "tr" => (Anchor::TOP | Anchor::RIGHT, (y, x, 0, 0)),
        "bl" => (Anchor::BOTTOM | Anchor::LEFT, (0, 0, y, x)),
        "br" => (Anchor::BOTTOM | Anchor::RIGHT, (0, x, y, 0)),
        _ => (Anchor::TOP | Anchor::LEFT, (y, 0, 0, x)),
    }
}

fn open_widget(app: &mut App, w: WidgetId, out: smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput) {
    let wc = w.cfg(&app.cfg);
    let size = widget_size(w, wc.scale.clamp(0.5, 2.0));
    let (anchor, margin) = widget_margin(app, w);
    app.open(Spec {
        kind: Kind::Widget(w),
        layer: Layer::Bottom,
        ns: "dshell-widget",
        anchor,
        size,
        exclusive: -1,
        margin,
        kb: KeyboardInteractivity::None,
        output: Some(out),
        topics: widget_topics(w),
        fps: app.cfg.widgets.fps.clamp(5, 60),
    });
}

/// Moves a widget while it is dragged in edit mode (no redraw: only the margins change).
pub fn move_widget(app: &mut App, w: WidgetId) {
    let (anchor, m) = widget_margin(app, w);
    if let Some(s) = app.surf_mut(Kind::Widget(w)) {
        s.layer.set_anchor(anchor);
        s.layer.set_margin(m.0, m.1, m.2, m.3);
        s.layer.commit();
    }
}

// ------------------------------------------------------------------ clock

fn clock_tick(app: &mut App) {
    let secs = app.cfg.bar.seconds || app.ui.focus.running;
    let d = crate::clock::until_next(secs);
    let _ = app.lh.insert_source(Timer::from_duration(d), |_, _, app: &mut App| {
        let secs = app.cfg.bar.seconds || app.ui.focus.running;
        let tm = crate::clock::now();
        let mut t = topic::SECOND;
        if tm.sec == 0 || app.cfg.bar.seconds {
            t |= topic::CLOCK;
        }
        if app.ui.focus.running {
            crate::actions::focus_tick(app);
        }
        app.mark(t);
        TimeoutAction::ToDuration(crate::clock::until_next(secs))
    });
}

// ------------------------------------------------------------------ config

struct Fd(RawFd);
impl AsFd for Fd {
    fn as_fd(&self) -> BorrowedFd<'_> {
        // SAFETY: the inotify descriptor lives for the whole process.
        unsafe { BorrowedFd::borrow_raw(self.0) }
    }
}

fn watch_config(app: &mut App) {
    let dir = crate::config::config_dir();
    let _ = std::fs::create_dir_all(&dir);
    // SAFETY: inotify syscalls on a path we own.
    let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
    if fd < 0 {
        return;
    }
    let c = std::ffi::CString::new(dir.to_string_lossy().as_bytes()).unwrap();
    // SAFETY: as above.
    unsafe {
        libc::inotify_add_watch(fd, c.as_ptr(), libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO | libc::IN_CREATE);
    }
    let _ = app.lh.insert_source(Generic::new(Fd(fd), Interest::READ, Mode::Level), move |_, _, app| {
        let mut buf = [0u8; 4096];
        // SAFETY: reading inotify events into our buffer.
        while unsafe { libc::read(fd, buf.as_mut_ptr() as *mut _, buf.len()) } > 0 {}
        if !app.svc.reload_armed {
            app.svc.reload_armed = true;
            let _ = app.lh.insert_source(Timer::from_duration(Duration::from_millis(120)), |_, _, app: &mut App| {
                app.svc.reload_armed = false;
                reload(app);
                TimeoutAction::Drop
            });
        }
        Ok(PostAction::Continue)
    });
}

pub fn reload(app: &mut App) {
    let (cfg, err) = crate::config::load();
    if let Some(e) = err {
        crate::actions::system_notice(app, "Config error", &e);
        return;
    }
    let wall_changed = cfg.wallpaper.path != app.cfg.wallpaper.path && !cfg.wallpaper.path.is_empty();
    app.cfg = cfg;
    app.theme = make_theme(&app.cfg, &app.st);
    build(app);
    crate::svc::viz::update_wanted(app);
    crate::svc::sysmon::update_wanted(app);
    if wall_changed {
        let p = crate::config::expand(&app.cfg.wallpaper.path);
        set_wallpaper(app, p, true);
    }
    app.mark(topic::ALL);
}

// ------------------------------------------------------------------ events from services

pub fn covered_changed(app: &mut App) {
    crate::svc::viz::update_wanted(app);
    crate::svc::sysmon::update_wanted(app);
    app.mark(topic::WM | topic::VIZ);
}

pub fn media_changed(app: &mut App) {
    crate::svc::viz::update_wanted(app);
}

// ------------------------------------------------------------------ wallpaper

pub fn wallpaper_list(app: &App) -> Vec<PathBuf> {
    let dir = crate::config::expand(&app.cfg.wallpaper.dir);
    let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    matches!(
                        p.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref(),
                        Some("jpg" | "jpeg" | "png")
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn current_wallpaper(app: &App) -> Option<PathBuf> {
    if !app.cfg.wallpaper.path.is_empty() {
        let p = crate::config::expand(&app.cfg.wallpaper.path);
        if p.is_file() {
            return Some(p);
        }
    }
    if !app.saved.wallpaper.is_empty() {
        let p = PathBuf::from(&app.saved.wallpaper);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(p) = wallpaper_list(app).into_iter().next() {
        return Some(p);
    }
    let shipped = crate::paths::share().join("wallpapers/default.jpg");
    shipped.is_file().then_some(shipped)
}

/// Decode for every screen, update the colors, fade it in.
pub fn set_wallpaper(app: &mut App, path: PathBuf, remember: bool) {
    if remember {
        app.saved.wallpaper = path.to_string_lossy().into_owned();
        crate::config::save_saved(&app.saved);
    }
    app.st.wall.path = path.to_string_lossy().into_owned();
    let mut targets: Vec<(u32, u32, u32)> = Vec::new();
    let mut waiting = false;
    for s in &app.surfs {
        if let Kind::Wall(id) = s.kind {
            if s.w <= 1 || s.h <= 1 {
                // not configured yet: its first draw asks again with the real size
                waiting = true;
                continue;
            }
            targets.push((id, s.w * s.scale as u32, s.h * s.scale as u32));
        }
    }
    if waiting && targets.is_empty() {
        return;
    }
    let main = app.svc.main_output.as_ref().map(|o| o.id().protocol_id());
    if targets.is_empty() {
        // no wallpaper surface (dshell does not draw it): still take the colors from it
        let (w, h) = (640, 360);
        targets.push((u32::MAX, w, h));
    }
    for (id, w, h) in targets {
        let p = path.clone();
        let (w, h) = (w.max(64), h.max(64));
        crate::proc::background(
            app,
            move || crate::svc::images::decode_wallpaper(&p, w, h),
            move |app, px| {
                let Some(px) = px else { return };
                if Some(id) == main || id == u32::MAX {
                    app.st.wall.lum = px.lum.clone();
                    app.st.wall.accent = Some(px.accent);
                    app.theme = make_theme(&app.cfg, &app.st);
                    app.mark(topic::ALL);
                }
                if id != u32::MAX {
                    crate::ui::wall::set(app, id, px);
                }
                trim_soon(app);
            },
        );
    }
}

pub fn trim_soon(app: &mut App) {
    let _ = app.lh.insert_source(Timer::from_duration(Duration::from_millis(1500)), |_, _, _app: &mut App| {
        // SAFETY: returns freed heap to the system; no preconditions.
        #[cfg(target_env = "gnu")]
        unsafe {
            libc::malloc_trim(0);
        }
        TimeoutAction::Drop
    });
}

