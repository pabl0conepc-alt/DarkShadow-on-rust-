//! Backends. Everything is pushed to us (sockets, D-Bus signals, `pactl subscribe`); nothing polls except the
//! resource monitor, and that only while something on screen shows it.

pub mod apps;
pub mod audio;
pub mod bus;
pub mod capture;
pub mod drift;
pub mod hypr;
pub mod images;
pub mod sysmon;
pub mod tray;
pub mod viz;
pub mod weather;

use std::any::Any;
use std::collections::HashMap;

use smithay_client_toolkit::reexports::calloop::channel::Sender;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;

use crate::app::App;
use crate::state::topic;

type BgDone = Box<dyn FnOnce(&mut App, Box<dyn Any + Send>)>;

#[derive(Default)]
pub struct Services {
    pub main_output: Option<WlOutput>,
    pub bg_tx: Option<Sender<(u64, Box<dyn Any + Send>)>>,
    pub bg_next: u64,
    pub bg_done: HashMap<u64, BgDone>,
    pub hypr: hypr::Hypr,
    pub drift: drift::Drift,
    pub audio: audio::AudioSvc,
    pub bus: bus::Bus,
    pub sysmon: sysmon::Sysmon,
    pub viz: viz::Viz,
    pub apps: apps::Apps,
    pub app_icons: HashMap<String, Option<std::rc::Rc<crate::gfx::Image>>>,
    pub app_icons_loading: std::collections::HashSet<String>,
    pub weather_at: Option<std::time::Instant>,
    pub weather_busy: bool,
    pub reload_armed: bool,
    pub capture: capture::Capture,
    pub tray: tray::Tray,
    pub sec_ticker: bool,
    pub clip_watch: Option<std::process::Child>,
    pub seek_preview: Option<f32>,
    pub island_hovered: bool,
    pub ptr_on_bar: bool,
    pub awake: Option<std::process::Child>,
    pub nightlight: Option<std::process::Child>,
}

pub fn start(app: &mut App) {
    hypr::start(app);
    if app.st.wm.kind == crate::state::WmKind::None {
        drift::start(app);
    }
    audio::start(app);
    bus::start(app);
    weather::start(app);
    sysmon::start(app);
    load_user(app);
    // the clipboard history: cliphist stores what you copy while the shell runs
    if crate::proc::which("cliphist") && crate::proc::which("wl-paste") && !crate::proc::running("wl-paste --watch cliphist") {
        app.svc.clip_watch = crate::proc::bound(&["wl-paste", "--watch", "cliphist", "store"]);
    }
    app.mark(topic::ALL);
}

/// Your name, the machine and your picture (~/.face or AccountsService).
pub fn load_user(app: &mut App) {
    let name = std::env::var("USER").unwrap_or_else(|_| "you".into());
    let mut real = String::new();
    if let Ok(pw) = std::fs::read_to_string("/etc/passwd") {
        for l in pw.lines() {
            let f: Vec<&str> = l.split(':').collect();
            if f.len() > 4 && f[0] == name {
                real = f[4].split(',').next().unwrap_or("").to_string();
            }
        }
    }
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim().to_string();
    let mut distro = String::new();
    if let Ok(osr) = std::fs::read_to_string("/etc/os-release") {
        for l in osr.lines() {
            if let Some(v) = l.strip_prefix("ID=") {
                distro = v.trim_matches('"').to_string();
            }
        }
    }
    app.st.user.name = name.clone();
    app.st.user.real = real;
    app.st.user.host = host;
    app.st.user.distro = distro;
    let home = crate::config::home();
    let cands = [
        home.join(".face"),
        home.join(".face.icon"),
        std::path::PathBuf::from(format!("/var/lib/AccountsService/icons/{name}")),
    ];
    if let Some(p) = cands.into_iter().find(|p| p.is_file()) {
        images::load_square(app, p.to_string_lossy().into_owned(), 128, |app, img| {
            app.st.user.avatar = img;
            app.mark(topic::USER);
        });
    }
}
