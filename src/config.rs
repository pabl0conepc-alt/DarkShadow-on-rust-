//! ~/.config/dshell/config.toml — every key optional; reloaded live when the file changes.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/tmp".into())
}

pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config")).join("dshell")
}

pub fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/state")).join("dshell")
}

pub fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".cache")).join("dshell")
}

pub fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| "/tmp".into())
}

pub fn expand(p: &str) -> PathBuf {
    if let Some(r) = p.strip_prefix("~/") { home().join(r) } else { PathBuf::from(p) }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    pub theme: ThemeCfg,
    pub bar: BarCfg,
    pub island: IslandCfg,
    pub wallpaper: WallCfg,
    pub widgets: WidgetsCfg,
    pub launcher: LauncherCfg,
    pub notifications: NotifCfg,
    pub control: ControlCfg,
    pub weather: WeatherCfg,
    pub apps: AppsCfg,
    pub performance: PerfCfg,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            theme: ThemeCfg::default(),
            bar: BarCfg::default(),
            island: IslandCfg::default(),
            wallpaper: WallCfg::default(),
            widgets: WidgetsCfg::default(),
            launcher: LauncherCfg::default(),
            notifications: NotifCfg::default(),
            control: ControlCfg::default(),
            weather: WeatherCfg::default(),
            apps: AppsCfg::default(),
            performance: PerfCfg::default(),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(default)]
pub struct ThemeCfg {
    /// auto (wallpaper), grey, navy, mono, slate, ..., violet; or a hex color
    pub scheme: String,
    /// shadow | google | outline (desktop widget look)
    pub widget_style: String,
    /// the compositor blurs behind the shell (Hyprland layer rules / driftwm blur)
    pub blur: bool,
    /// no transparency anywhere
    pub flat: bool,
    /// how solid the panels are
    pub panel_alpha: f32,
    /// your shape: icons, loaders and highlights take it
    pub shape: String,
    /// wavy progress lines
    pub wavy: bool,
    /// widgets turn light or dark with the wallpaper behind them
    pub adapt: bool,
    pub shadows: bool,
}

impl Default for ThemeCfg {
    fn default() -> Self {
        ThemeCfg {
            scheme: "auto".into(),
            widget_style: "google".into(),
            blur: true,
            flat: false,
            panel_alpha: 0.9,
            shape: "verysunny".into(),
            wavy: true,
            adapt: true,
            shadows: true,
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(default)]
pub struct BarCfg {
    pub enabled: bool,
    /// top | bottom
    pub position: String,
    /// island (one pill in the middle) | dynamic (tonal pills on each side) | flat (full width)
    pub style: String,
    pub height: u32,
    pub opacity: f32,
    pub left: Vec<String>,
    pub center: Vec<String>,
    pub right: Vec<String>,
    /// how many workspaces are shown
    pub workspaces: u32,
    /// dots | numbers
    pub ws_style: String,
    pub seconds: bool,
    pub date: bool,
    pub autohide: bool,
    /// launcher button: auto (distro logo) | apps
    pub logo: String,
    /// the output the bar (and the widgets) live on; empty = the first one
    pub output: String,
}

impl Default for BarCfg {
    fn default() -> Self {
        BarCfg {
            enabled: true,
            position: "bottom".into(),
            style: "island".into(),
            height: 38,
            opacity: 0.72,
            left: vec!["power".into(), "launcher".into(), "workspaces".into()],
            center: vec!["island".into()],
            right: vec!["tray".into(), "network".into(), "bluetooth".into(), "volume".into(), "battery".into(), "notifications".into()],
            workspaces: 5,
            ws_style: "dots".into(),
            seconds: false,
            date: false,
            autohide: false,
            logo: "auto".into(),
            output: String::new(),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(default)]
pub struct IslandCfg {
    /// off: only the clock (a click still opens Today)
    pub dynamic: bool,
    pub media: bool,
    pub volume: bool,
    pub workspaces: bool,
    pub notifications: bool,
    pub charger: bool,
    pub weather: bool,
    /// the little visualizer wave next to the time while music plays
    pub wave: bool,
}

impl Default for IslandCfg {
    fn default() -> Self {
        IslandCfg { dynamic: true, media: true, volume: true, workspaces: true, notifications: true, charger: true, weather: true, wave: true }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(default)]
pub struct WallCfg {
    /// dshell draws the wallpaper itself (one buffer per screen). Off when you use swww/hyprpaper/driftwm's own.
    pub enabled: bool,
    pub path: String,
    pub dir: String,
    /// fade | none
    pub transition: String,
}

impl Default for WallCfg {
    fn default() -> Self {
        WallCfg { enabled: true, path: String::new(), dir: "~/Pictures/Wallpapers".into(), transition: "fade".into() }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(default)]
pub struct WidgetCfg {
    pub enabled: bool,
    /// tl | tr | bl | br (the corner its position is measured from)
    pub anchor: String,
    pub x: i32,
    pub y: i32,
    pub style: String,
    pub scale: f32,
}

impl WidgetCfg {
    fn new(enabled: bool, anchor: &str, x: i32, y: i32, style: &str) -> Self {
        WidgetCfg { enabled, anchor: anchor.into(), x, y, style: style.into(), scale: 1.0 }
    }
}

impl Default for WidgetCfg {
    fn default() -> Self {
        WidgetCfg::new(false, "tl", 100, 100, "")
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(default)]
pub struct WidgetsCfg {
    pub enabled: bool,
    /// moving parts (turning shapes, the visualizer) at most this many frames a second
    pub fps: u32,
    /// pause the widgets' motion while windows cover the desktop
    pub pause_covered: bool,
    pub clock: WidgetCfg,
    pub user: WidgetCfg,
    pub trio: WidgetCfg,
    pub focus: WidgetCfg,
    pub player: WidgetCfg,
    pub weather: WidgetCfg,
    pub calendar: WidgetCfg,
    pub visualizer: VizCfg,
}

impl Default for WidgetsCfg {
    fn default() -> Self {
        WidgetsCfg {
            enabled: true,
            fps: 20,
            pause_covered: true,
            clock: WidgetCfg::new(true, "tl", 64, 56, "expressive"),
            user: WidgetCfg::new(true, "tl", 28, 414, ""),
            trio: WidgetCfg::new(true, "bl", 52, 104, ""),
            focus: WidgetCfg::new(true, "tr", 142, 198, ""),
            player: WidgetCfg::new(true, "br", 94, 90, ""),
            weather: WidgetCfg::new(false, "tr", 142, 520, ""),
            calendar: WidgetCfg::new(false, "tl", 420, 60, ""),
            visualizer: VizCfg::default(),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(default)]
pub struct VizCfg {
    pub enabled: bool,
    pub height: u32,
    pub bars: u32,
    /// bars | mirror | wave | dots
    pub style: String,
    /// accent | gradient | rainbow | mono
    pub color: String,
    pub fps: u32,
}

impl Default for VizCfg {
    fn default() -> Self {
        VizCfg { enabled: true, height: 90, bars: 64, style: "bars".into(), color: "accent".into(), fps: 30 }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(default)]
pub struct LauncherCfg {
    pub width: u32,
    pub rows: u32,
    /// top | center | bottom
    pub position: String,
    pub terminal: String,
}

impl Default for LauncherCfg {
    fn default() -> Self {
        LauncherCfg { width: 560, rows: 7, position: "center".into(), terminal: "kitty".into() }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(default)]
pub struct NotifCfg {
    pub enabled: bool,
    /// seconds a pop-up stays
    pub timeout: u32,
    pub dnd: bool,
    pub history: u32,
}

impl Default for NotifCfg {
    fn default() -> Self {
        NotifCfg { enabled: true, timeout: 6, dnd: false, history: 50 }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(default)]
pub struct ControlCfg {
    pub width: u32,
    /// commands for the quick switches
    pub nightlight_on: String,
    pub nightlight_off: String,
    pub settings: String,
    pub lock: String,
}

impl Default for ControlCfg {
    fn default() -> Self {
        ControlCfg {
            width: 440,
            nightlight_on: String::new(),
            nightlight_off: String::new(),
            settings: String::new(),
            lock: String::new(),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(default)]
pub struct WeatherCfg {
    pub enabled: bool,
    /// a city name, or empty for where your IP is
    pub location: String,
    /// c | f
    pub units: String,
    pub minutes: u32,
}

impl Default for WeatherCfg {
    fn default() -> Self {
        WeatherCfg { enabled: true, location: String::new(), units: "c".into(), minutes: 30 }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(default)]
pub struct AppsCfg {
    pub terminal: String,
    pub files: String,
    pub browser: String,
    /// icon theme for app icons; empty: the GTK one
    pub icon_theme: String,
}

impl Default for AppsCfg {
    fn default() -> Self {
        AppsCfg { terminal: "kitty".into(), files: "nautilus".into(), browser: "firefox".into(), icon_theme: String::new() }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(default)]
pub struct PerfCfg {
    /// return freed memory to the system after closing panels
    pub trim_memory: bool,
    /// gaming mode: no animations, no visualizer, no widget motion
    pub gaming: bool,
}

impl Default for PerfCfg {
    fn default() -> Self {
        PerfCfg { trim_memory: true, gaming: false }
    }
}

pub fn path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn load() -> (Config, Option<String>) {
    let p = path();
    match std::fs::read_to_string(&p) {
        Ok(s) => match toml::from_str::<Config>(&s) {
            Ok(c) => (c, None),
            Err(e) => (Config::default(), Some(format!("{}: {e}", p.display()))),
        },
        Err(_) => (Config::default(), None),
    }
}

/// Small runtime state that survives restarts (the wallpaper, do-not-disturb ...).
#[derive(Deserialize, Serialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Saved {
    pub wallpaper: String,
    pub scheme: String,
    pub dnd: bool,
    pub focus_minutes: u32,
    pub app_usage: std::collections::HashMap<String, u32>,
}

pub fn load_saved() -> Saved {
    std::fs::read_to_string(state_dir().join("state.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_saved(s: &Saved) {
    let dir = state_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(t) = serde_json::to_string(s) {
        let tmp = dir.join("state.json.tmp");
        if std::fs::write(&tmp, t).is_ok() {
            let _ = std::fs::rename(tmp, dir.join("state.json"));
        }
    }
}

pub const DEFAULT_TOML: &str = include_str!("../config/dshell/config.toml");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_parses_and_matches_defaults() {
        let c: Config = toml::from_str(DEFAULT_TOML).expect("example config must parse");
        let d = Config::default();
        assert_eq!(c.bar.left, d.bar.left);
        assert_eq!(c.bar.right, d.bar.right);
        assert_eq!(c.widgets.fps, d.widgets.fps);
        assert_eq!(c.widgets.player.anchor, d.widgets.player.anchor);
        assert_eq!(c.widgets.visualizer.fps, d.widgets.visualizer.fps);
        assert_eq!(c.theme.scheme, d.theme.scheme);
    }

    #[test]
    fn partial_config_keeps_defaults() {
        let c: Config = toml::from_str("[bar]\nposition = \"top\"\n").unwrap();
        assert_eq!(c.bar.position, "top");
        assert_eq!(c.bar.height, 38);
        assert!(c.widgets.clock.enabled);
    }
}
