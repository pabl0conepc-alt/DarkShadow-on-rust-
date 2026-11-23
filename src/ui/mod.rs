//! The panels of the shell and what they share: which surface is which, the actions a click can do, the hit
//! areas a draw pass leaves behind, and the per-panel UI state.

pub mod bar;
pub mod card;
pub mod control;
pub mod launcher;
pub mod picker;
pub mod settings;
pub mod power;
pub mod wall;
pub mod widgets;

use std::time::Instant;

pub use crate::actions::{
    act, drag_end, drag_motion, focus_lost, hover, key, on_enter, press_empty, scroll, scroll_surface, slide, slide_end,
};

use crate::anim::{Glide, Reveal, Spring};
use crate::color::Rgba;
use crate::config::Config;
use crate::gfx::{Align, Canvas};
use crate::state::State;
use crate::text::Face;
use crate::theme::Theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum WidgetId {
    Clock,
    User,
    Trio,
    Focus,
    Player,
    Weather,
    Calendar,
}

pub const WIDGETS: [WidgetId; 7] =
    [WidgetId::Clock, WidgetId::User, WidgetId::Trio, WidgetId::Focus, WidgetId::Player, WidgetId::Weather, WidgetId::Calendar];

impl WidgetId {
    pub fn name(self) -> &'static str {
        match self {
            WidgetId::Clock => "clock",
            WidgetId::User => "user",
            WidgetId::Trio => "trio",
            WidgetId::Focus => "focus",
            WidgetId::Player => "player",
            WidgetId::Weather => "weather",
            WidgetId::Calendar => "calendar",
        }
    }
    pub fn cfg(self, c: &Config) -> &crate::config::WidgetCfg {
        let w = &c.widgets;
        match self {
            WidgetId::Clock => &w.clock,
            WidgetId::User => &w.user,
            WidgetId::Trio => &w.trio,
            WidgetId::Focus => &w.focus,
            WidgetId::Player => &w.player,
            WidgetId::Weather => &w.weather,
            WidgetId::Calendar => &w.calendar,
        }
    }
    pub fn cfg_mut(self, c: &mut Config) -> &mut crate::config::WidgetCfg {
        let w = &mut c.widgets;
        match self {
            WidgetId::Clock => &mut w.clock,
            WidgetId::User => &mut w.user,
            WidgetId::Trio => &mut w.trio,
            WidgetId::Focus => &mut w.focus,
            WidgetId::Player => &mut w.player,
            WidgetId::Weather => &mut w.weather,
            WidgetId::Calendar => &mut w.calendar,
        }
    }
}

/// Which surface a layer surface is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind {
    Wall(u32),
    Bar,
    Card,
    Launcher,
    Control,
    Power,
    Picker,
    Settings,
    Widget(WidgetId),
    Viz,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardMode {
    Today,
    System,
    Player,
    Notif,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerAct {
    Lock,
    Suspend,
    Logout,
    Reboot,
    Shutdown,
}

/// What a click (or a drag on a slider) does.
#[derive(Clone, PartialEq, Debug)]
#[allow(dead_code)]
pub enum Act {
    None,
    OpenLauncher,
    ToggleControl,
    TogglePower,
    OpenCard(CardMode),
    CloseAll,
    Ws(i32),
    WsRel(i32),
    PlayPause,
    Next,
    Prev,
    SeekRel(f64),
    /// sliders: the value comes from where the pointer is
    Seek,
    Volume,
    Mic,
    Bright,
    Mute,
    MicMute,
    Wifi,
    Bluetooth,
    Dnd,
    NightLight,
    Awake,
    Airplane,
    Gaming,
    NotifDismiss(u32),
    NotifAction(u32, String),
    NotifOpen(u32),
    NotifClear,
    Launch(usize),
    Power(PowerAct),
    FocusToggle,
    FocusReset,
    FocusSkip,
    FocusMode(bool),
    FocusMinutes(i32),
    CardTab(u8),
    ControlTab(u8),
    Wallpaper(usize),
    WallShuffle,
    OpenPicker,
    Settings,
    OpenConfigFile,
    Reload,
    NotifTest,
    EditMode,
    SettingsPage(u8),
    CfgToggle(&'static str),
    CfgSet(&'static str, String),
    CfgSlide(&'static str),
    CcView(u8),
    RecStop,
    TrayClick(String),
    TrayMenu(String),
    TrayAlt(String),
    WifiPick(String),
    WifiScan,
    BtPick(String),
    BtScan,
    PwSubmit,
    PwCancel,
    Run(String),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HitKind {
    Click,
    /// a horizontal slider between x0 and x1
    Slider,
}

#[derive(Clone, Debug)]
pub struct Hit {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub act: Act,
    pub right: Act,
    pub scroll: Act,
    pub kind: HitKind,
    pub id: u32,
}

impl Hit {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    pub fn frac(&self, x: f32) -> f32 {
        ((x - self.x) / self.w.max(1.0)).clamp(0.0, 1.0)
    }
}

/// Collects hit areas while drawing; ids are stable as long as the layout is.
pub struct Hits {
    pub list: Vec<Hit>,
    /// ids start after this (a cached layer's hits come first)
    pub base: u32,
    pub hover: Option<u32>,
    pub pressed: Option<u32>,
    /// translation of the canvas when the hits are added
    pub ox: f32,
    pub oy: f32,
}

impl Hits {
    pub fn new(hover: Option<u32>, pressed: Option<u32>) -> Self {
        Hits { list: Vec::new(), base: 0, hover, pressed, ox: 0.0, oy: 0.0 }
    }
    fn push(&mut self, x: f32, y: f32, w: f32, h: f32, act: Act, kind: HitKind) -> u32 {
        let id = self.base + self.list.len() as u32 + 1;
        self.list.push(Hit { x: x + self.ox, y: y + self.oy, w, h, act, right: Act::None, scroll: Act::None, kind, id });
        id
    }
    /// add a clickable area; returns whether the pointer is on it
    pub fn add(&mut self, x: f32, y: f32, w: f32, h: f32, act: Act) -> bool {
        let id = self.push(x, y, w, h, act, HitKind::Click);
        self.hover == Some(id)
    }
    pub fn slider(&mut self, x: f32, y: f32, w: f32, h: f32, act: Act) -> bool {
        let id = self.push(x, y, w, h, act, HitKind::Slider);
        self.hover == Some(id) || self.pressed == Some(id)
    }
    /// the last area added also reacts to the right button / the wheel
    pub fn with_right(&mut self, act: Act) {
        if let Some(h) = self.list.last_mut() {
            h.right = act;
        }
    }
    pub fn with_scroll(&mut self, act: Act) {
        if let Some(h) = self.list.last_mut() {
            h.scroll = act;
        }
    }
    pub fn is_pressed_last(&self) -> bool {
        self.list.last().map(|h| Some(h.id) == self.pressed).unwrap_or(false)
    }
}

/// Everything a draw pass needs to read.
pub struct Ctx<'a> {
    pub cfg: &'a Config,
    pub st: &'a State,
    pub th: &'a Theme,
    pub now: Instant,
    /// screen size (logical) of the output the shell lives on
    pub sw: f32,
    pub sh: f32,
    /// where the seek line is being dragged to
    pub seek: Option<f32>,
    pub edit: bool,
    pub apps: &'a [crate::svc::apps::Entry],
    pub app_icons: &'a std::collections::HashMap<String, Option<std::rc::Rc<crate::gfx::Image>>>,
}

/// What a draw pass reports back.
#[derive(Default)]
pub struct Out {
    /// wants another frame (an animation runs)
    pub anim: bool,
    /// the parts of the surface that take the pointer (None: all of it)
    pub input: Option<Vec<(f32, f32, f32, f32)>>,
    /// the panel finished closing: destroy its surface
    pub done: bool,
    /// only these parts changed since the last frame (None: everything)
    pub damage: Option<Vec<(f32, f32, f32, f32)>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IslandMode {
    Idle,
    Volume,
    Mic,
    Bright,
    Ws,
    Charger,
}

pub struct IslandUi {
    pub mode: IslandMode,
    pub mode_at: Instant,
    pub width: Spring,
    /// cross-fade between faces
    pub face: Glide,
    pub pulse: Option<Instant>,
}

pub struct CardUi {
    pub mode: CardMode,
    pub tab: u8,
    pub rev: Reveal,
    /// opened with a click (stays until Esc / click elsewhere) vs. by hover / a notification (goes by itself)
    pub pinned: bool,
    pub hovered: bool,
    pub left_at: Option<Instant>,
    pub notif: Option<u32>,
}

pub struct LauncherUi {
    pub query: String,
    pub sel: usize,
    pub top: usize,
    pub results: Vec<usize>,
    pub rev: Reveal,
}

pub struct ControlUi {
    pub tab: u8,
    pub rev: Reveal,
    pub scroll: f32,
    /// 0 the switches, 1 the Wi-Fi list, 2 the Bluetooth list
    pub view: u8,
    /// typing the password for this network
    pub pw_for: Option<String>,
    pub pw: String,
}

pub struct PickerUi {
    pub sel: usize,
    pub list: Vec<std::path::PathBuf>,
    pub thumbs: std::collections::HashMap<usize, std::rc::Rc<crate::gfx::Image>>,
    pub loading: std::collections::HashSet<usize>,
    pub rev: Reveal,
}

pub struct SettingsUi {
    pub page: u8,
    pub scroll: f32,
    pub rev: Reveal,
    /// keys changed but not yet written to the file
    pub pending: Vec<(String, toml::Value)>,
    pub write_armed: bool,
}

pub struct FocusUi {
    pub running: bool,
    pub on_break: bool,
    pub minutes: u32,
    pub break_minutes: u32,
    /// seconds left (while paused) / the end time (while running)
    pub left: f64,
    pub end: Option<Instant>,
    pub done_sessions: u32,
}

impl FocusUi {
    pub fn remaining(&self, now: Instant) -> f64 {
        match self.end {
            Some(e) if self.running => e.saturating_duration_since(now).as_secs_f64(),
            _ => self.left,
        }
    }
    pub fn total(&self) -> f64 {
        (if self.on_break { self.break_minutes } else { self.minutes }) as f64 * 60.0
    }
}

pub struct Ui {
    pub island: IslandUi,
    pub card: CardUi,
    pub launcher: LauncherUi,
    pub control: ControlUi,
    pub power: Reveal,
    pub power_sel: usize,
    pub picker: PickerUi,
    pub settings: SettingsUi,
    pub focus: FocusUi,
    pub bar_rev: Reveal,
    pub edit: bool,
    pub walls: wall::Walls,
    pub wall_redecode: bool,
    /// app icons a draw pass wanted but were not loaded yet
    pub want_icons: Vec<String>,
    pub want_thumbs: Vec<usize>,
    /// a widget being dragged in edit mode: (widget, pointer start, config x/y at start)
    pub drag: Option<(WidgetId, (f32, f32), (i32, i32))>,
}

impl Ui {
    pub fn new() -> Self {
        let now = Instant::now();
        Ui {
            island: IslandUi {
                mode: IslandMode::Idle,
                mode_at: now,
                width: Spring::new(120.0),
                face: Glide::new(1.0, 180),
                pulse: None,
            },
            card: CardUi {
                mode: CardMode::Today,
                tab: 0,
                rev: Reveal::new(420, 220),
                pinned: false,
                hovered: false,
                left_at: None,
                notif: None,
            },
            launcher: LauncherUi { query: String::new(), sel: 0, top: 0, results: Vec::new(), rev: Reveal::new(320, 160) },
            control: ControlUi { tab: 0, rev: Reveal::new(380, 200), scroll: 0.0, view: 0, pw_for: None, pw: String::new() },
            power: Reveal::new(300, 160),
            power_sel: 0,
            settings: SettingsUi { page: 0, scroll: 0.0, rev: Reveal::new(340, 180), pending: Vec::new(), write_armed: false },
            picker: PickerUi {
                sel: 0,
                list: Vec::new(),
                thumbs: Default::default(),
                loading: Default::default(),
                rev: Reveal::new(340, 180),
            },
            focus: FocusUi { running: false, on_break: false, minutes: 25, break_minutes: 5, left: 25.0 * 60.0, end: None, done_sessions: 0 },
            bar_rev: Reveal::new(260, 200),
            edit: false,
            walls: Default::default(),
            wall_redecode: false,
            want_icons: Vec::new(),
            want_thumbs: Vec::new(),
            drag: None,
        }
    }
}

// ---------------------------------------------------------------- shared pieces

static EPOCH: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);

/// Seconds since the shell started (phases of the decorative motion).
pub fn secs(now: Instant) -> f32 {
    (now.saturating_duration_since(*EPOCH).as_secs_f64() % 100_000.0) as f32
}

/// A key for a cached still layer: anything that changes how the still part looks.
pub fn hkey<T: std::hash::Hash>(v: &T) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

pub fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// A round icon button; returns whether hovered.
#[allow(clippy::too_many_arguments)]
pub fn icon_button(
    cv: &mut Canvas,
    hits: &mut Hits,
    cx: f32,
    cy: f32,
    r: f32,
    icon: &str,
    fg: Rgba,
    bg: Rgba,
    hover_bg: Rgba,
    act: Act,
) -> bool {
    let hov = hits.add(cx - r, cy - r, r * 2.0, r * 2.0, act);
    let pressed = hits.is_pressed_last();
    let b = if hov { hover_bg } else { bg };
    let rr = if pressed { r * 0.92 } else { r };
    if b.a > 0.0 {
        cv.circle(cx, cy, rr, b);
    }
    cv.icon(icon, cx, cy, r * 1.05, fg);
    hov
}

/// A pill-shaped slider with its icon inside (control center, island volume).
#[allow(clippy::too_many_arguments)]
pub fn pill_slider(cv: &mut Canvas, hits: &mut Hits, th: &Theme, x: f32, y: f32, w: f32, h: f32, v: f32, icon: &str, off: bool, act: Act) {
    let p = &th.pal;
    let active = hits.slider(x, y, w, h, act);
    cv.rrect(x, y, w, h, h / 2.0, p.card);
    let fill = (h).max(w * v.clamp(0.0, 1.0));
    cv.rrect(x, y, fill, h, h / 2.0, if off { p.card_hi } else { p.primary });
    if active {
        cv.rrect(x, y, w, h, h / 2.0, crate::color::rgba(1.0, 1.0, 1.0, 0.04));
    }
    cv.icon(icon, x + h / 2.0, y + h / 2.0, h * 0.46, if off { p.fg } else { p.ink_primary });
    let pct = format!("{}", (v * 100.0).round() as i32);
    let ink = if fill > w - 36.0 && !off { p.ink_primary } else { p.dim };
    cv.text(x + w - 16.0, y + h / 2.0, &pct, Face::Semi, 12.0, ink, Align::Right);
}


pub fn weekday(i: u32) -> &'static str {
    ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"][(i % 7) as usize]
}

pub fn month(i: u32) -> &'static str {
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"]
        [(i % 12) as usize]
}
