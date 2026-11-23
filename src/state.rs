//! What the services know. The UI only reads this.

use std::rc::Rc;
use std::time::Instant;

use crate::gfx::Image;

/// Topics: a service marks one when its data changed; surfaces redraw when a topic they use is marked.
pub mod topic {
    pub const WM: u32 = 1;
    pub const AUDIO: u32 = 1 << 1;
    pub const MEDIA: u32 = 1 << 2;
    pub const NET: u32 = 1 << 3;
    pub const BT: u32 = 1 << 4;
    pub const BATT: u32 = 1 << 5;
    pub const NOTIF: u32 = 1 << 6;
    pub const SYS: u32 = 1 << 7;
    pub const WEATHER: u32 = 1 << 8;
    pub const CLOCK: u32 = 1 << 9;
    pub const THEME: u32 = 1 << 10;
    pub const VIZ: u32 = 1 << 11;
    pub const APPS: u32 = 1 << 12;
    pub const TOGGLES: u32 = 1 << 13;
    pub const SECOND: u32 = 1 << 14;
    pub const USER: u32 = 1 << 15;
    pub const REC: u32 = 1 << 16;
    pub const TRAY: u32 = 1 << 17;
    pub const ALL: u32 = !0;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum WmKind {
    #[default]
    None,
    Hyprland,
    Drift,
}

#[derive(Clone, Debug, Default)]
pub struct Ws {
    pub id: i32,
    pub name: String,
    pub windows: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Wm {
    pub kind: WmKind,
    pub workspaces: Vec<Ws>,
    pub active: i32,
    pub title: String,
    pub class: String,
    /// windows cover the desktop (the widgets pause)
    pub covered: bool,
    pub fullscreen: bool,
    pub layout: String,
}

#[derive(Clone, Debug, Default)]
pub struct Audio {
    pub ok: bool,
    pub vol: f32,
    pub muted: bool,
    pub mic: f32,
    pub mic_muted: bool,
}

#[derive(Clone, Default)]
pub struct Media {
    pub bus: String,
    pub identity: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: String,
    pub art: Option<Rc<Image>>,
    pub track_id: String,
    pub length_us: i64,
    pub pos_us: i64,
    pub pos_at: Option<Instant>,
    pub playing: bool,
    pub can_seek: bool,
}

impl Media {
    pub fn active(&self) -> bool {
        !self.bus.is_empty() && (!self.title.is_empty() || self.playing)
    }
    pub fn position(&self, now: Instant) -> f64 {
        let mut p = self.pos_us as f64 / 1e6;
        if self.playing
            && let Some(at) = self.pos_at
        {
            p += now.saturating_duration_since(at).as_secs_f64();
        }
        if self.length_us > 0 { p.min(self.length_us as f64 / 1e6) } else { p }
    }
    pub fn fraction(&self, now: Instant) -> f32 {
        if self.length_us <= 0 {
            return 0.0;
        }
        (self.position(now) / (self.length_us as f64 / 1e6)).clamp(0.0, 1.0) as f32
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NetKind {
    #[default]
    None,
    Wifi,
    Wired,
}

#[derive(Clone, Debug, Default)]
pub struct Net {
    pub ok: bool,
    pub wifi_on: bool,
    pub kind: NetKind,
    pub ssid: String,
    pub strength: u8,
    /// networks in reach (filled while the Wi-Fi list is open)
    pub aps: Vec<Ap>,
    pub connecting: String,
}

#[derive(Clone, Debug, Default)]
pub struct Ap {
    pub ssid: String,
    pub strength: u8,
    pub secured: bool,
    /// a saved connection exists (no password needed)
    pub saved: bool,
    pub active: bool,
    pub path: String,
}

impl Net {
    pub fn icon(&self) -> &'static str {
        match self.kind {
            NetKind::Wired => "lan",
            NetKind::Wifi => match self.strength {
                0..=24 => "network_wifi_1_bar",
                25..=49 => "network_wifi_2_bar",
                50..=74 => "network_wifi_3_bar",
                _ => "signal_wifi_4_bar",
            },
            NetKind::None => {
                if self.wifi_on {
                    "wifi"
                } else {
                    "wifi_off"
                }
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Bt {
    pub present: bool,
    pub powered: bool,
    pub connected: Vec<String>,
    pub devices: Vec<BtDev>,
    pub scanning: bool,
    pub busy: String,
    /// whether the busy device was connected when we asked (so we know when it has changed)
    pub busy_from: bool,
}

#[derive(Clone, Debug, Default)]
pub struct BtDev {
    pub path: String,
    pub name: String,
    pub icon: String,
    pub paired: bool,
    pub connected: bool,
    pub battery: Option<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct Batt {
    pub present: bool,
    pub pct: f32,
    pub charging: bool,
    pub plugged: bool,
}

impl Batt {
    pub fn icon(&self) -> &'static str {
        if self.charging {
            return "battery_charging_full";
        }
        match self.pct as u32 {
            0..=10 => "battery_0_bar",
            11..=24 => "battery_1_bar",
            25..=38 => "battery_2_bar",
            39..=52 => "battery_3_bar",
            53..=66 => "battery_4_bar",
            67..=80 => "battery_5_bar",
            81..=94 => "battery_6_bar",
            _ => "battery_full",
        }
    }
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct Notif {
    pub id: u32,
    pub app: String,
    pub summary: String,
    pub body: String,
    pub icon: String,
    pub image: Option<Rc<Image>>,
    pub actions: Vec<(String, String)>,
    pub urgency: u8,
    pub timeout_ms: i32,
    pub at: Instant,
    pub wall: std::time::SystemTime,
}

#[derive(Clone, Default)]
pub struct Notifs {
    pub list: Vec<Notif>,
    /// the one showing as a pop-up (in the island)
    pub popup: Option<u32>,
    pub popup_at: Option<Instant>,
    pub next_id: u32,
    pub owned: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Sys {
    pub cpu: f32,
    pub ram: f32,
    pub ram_used: f32,
    pub ram_total: f32,
    pub gpu: Option<f32>,
    pub temp: Option<f32>,
    pub disk: f32,
    pub disk_used: f32,
    pub disk_total: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Weather {
    pub ok: bool,
    pub temp: f32,
    pub feels: f32,
    pub desc: String,
    pub icon: &'static str,
    pub city: String,
    pub min: f32,
    pub max: f32,
    pub humidity: u32,
    pub wind: f32,
    pub unit: char,
    pub hours: Vec<(String, f32, &'static str)>,
}

#[derive(Clone, Debug, Default)]
pub struct Toggles {
    pub dnd: bool,
    pub nightlight: bool,
    pub awake: bool,
    pub airplane: bool,
    pub gaming: bool,
}

#[derive(Clone, Default)]
pub struct User {
    pub name: String,
    pub real: String,
    pub host: String,
    pub distro: String,
    pub avatar: Option<Rc<Image>>,
}

#[derive(Clone, Debug, Default)]
pub struct Bright {
    pub ok: bool,
    pub pct: f32,
    pub device: String,
    pub max: u32,
}

#[derive(Clone, Default)]
pub struct Wall {
    pub path: String,
    /// 64 x 36 cells of the wallpaper's brightness, 0..1
    pub lum: Vec<f32>,
    pub accent: Option<crate::color::Rgba>,
}

impl Wall {
    pub const LW: usize = 64;
    pub const LH: usize = 36;
    /// average brightness of a part of the screen (screen px)
    pub fn lum_at(&self, x: f32, y: f32, w: f32, h: f32, sw: f32, sh: f32) -> f32 {
        if self.lum.len() < Self::LW * Self::LH || sw <= 0.0 || sh <= 0.0 {
            return 0.3;
        }
        let cx = |v: f32| ((v / sw * Self::LW as f32) as isize).clamp(0, Self::LW as isize - 1) as usize;
        let cy = |v: f32| ((v / sh * Self::LH as f32) as isize).clamp(0, Self::LH as isize - 1) as usize;
        let (x0, x1, y0, y1) = (cx(x), cx(x + w), cy(y), cy(y + h));
        let mut s = 0.0;
        let mut n = 0.0;
        for j in y0..=y1 {
            for i in x0..=x1 {
                s += self.lum[j * Self::LW + i];
                n += 1.0;
            }
        }
        if n > 0.0 { s / n } else { 0.3 }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Rec {
    pub on: bool,
    pub since: Option<Instant>,
    pub path: String,
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct TrayIcon {
    pub key: String,
    pub title: String,
    pub icon: Option<Rc<Image>>,
    pub letter: String,
}

#[derive(Default)]
pub struct State {
    pub rec: Rec,
    pub tray: Vec<TrayIcon>,
    pub wm: Wm,
    pub audio: Audio,
    pub media: Media,
    pub net: Net,
    pub bt: Bt,
    pub batt: Batt,
    pub notifs: Notifs,
    pub sys: Sys,
    pub weather: Weather,
    pub toggles: Toggles,
    pub user: User,
    pub bright: Bright,
    pub wall: Wall,
    /// visualizer bars 0..1
    pub viz: Vec<f32>,
    pub viz_on: bool,
}
