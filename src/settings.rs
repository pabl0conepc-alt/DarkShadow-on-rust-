//! The Settings app (SUPER + U): every option of config.toml as switches, choices and sliders.
//! A change applies to the running shell at once; only the keys you touched are written back to your file
//! (comments and the rest of it stay), a moment after you stop changing them.

use std::time::Duration;

use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};

use crate::app::{App, make_theme};
use crate::config::Config;
use crate::state::topic;
use crate::ui::Act;

pub enum Ctl {
    Switch,
    Choice(&'static [(&'static str, &'static str)]),
    Slider { min: f64, max: f64, step: f64, int: bool },
    /// the color schemes as chips
    Scheme,
    /// the shapes as chips
    Shape,
    /// a button that does something
    Button(&'static str, fn() -> Act),
    /// a line of information
    Info,
}

pub struct Row {
    /// config key ("bar.position"); "" for a section title
    pub key: &'static str,
    pub title: &'static str,
    pub desc: &'static str,
    pub ctl: Ctl,
}

pub struct Page {
    pub icon: &'static str,
    pub name: &'static str,
    pub sub: &'static str,
    pub rows: &'static [Row],
}

const fn sec(title: &'static str, desc: &'static str) -> Row {
    Row { key: "", title, desc, ctl: Ctl::Info }
}
const fn sw(key: &'static str, title: &'static str, desc: &'static str) -> Row {
    Row { key, title, desc, ctl: Ctl::Switch }
}
const fn ch(key: &'static str, title: &'static str, desc: &'static str, opts: &'static [(&'static str, &'static str)]) -> Row {
    Row { key, title, desc, ctl: Ctl::Choice(opts) }
}
const fn sl(key: &'static str, title: &'static str, desc: &'static str, min: f64, max: f64, step: f64, int: bool) -> Row {
    Row { key, title, desc, ctl: Ctl::Slider { min, max, step, int } }
}

pub const PAGES: &[Page] = &[
    Page {
        icon: "palette",
        name: "Appearance",
        sub: "Colors, shapes and glass",
        rows: &[
            sec("Colors", "Auto takes them from your wallpaper; a black, white or grey picture gives grey colors"),
            Row { key: "theme.scheme", title: "Color scheme", desc: "", ctl: Ctl::Scheme },
            ch(
                "theme.widget_style",
                "Widget style",
                "How the desktop widgets look",
                &[("google", "Google"), ("shadow", "DarkShadow"), ("outline", "Outline")],
            ),
            sw("theme.adapt", "Follow the wallpaper", "Widgets turn light or dark with the picture behind each one"),
            sec("Shapes", "Your shape: avatars, highlights, the selected day"),
            Row { key: "theme.shape", title: "Your shape", desc: "", ctl: Ctl::Shape },
            sw("theme.wavy", "Wavy progress", "Material 3 waves on progress lines and rings"),
            sec("Glass", "Blur needs the compositor rules from config/ (Hyprland or driftwm)"),
            sw("theme.blur", "Blur", "The compositor blurs the wallpaper behind the cards"),
            sl("theme.panel_alpha", "Panel transparency", "Lower is more see-through", 0.3, 1.0, 0.02, false),
            sw("theme.shadows", "Shadows", "A soft shadow under cards and the bar"),
            sw("theme.flat", "No transparency", "Solid surfaces everywhere"),
        ],
    },
    Page {
        icon: "toolbar",
        name: "Bar",
        sub: "The DarkShadow bar",
        rows: &[
            sw("bar.enabled", "Show the bar", ""),
            ch("bar.position", "Position", "Top or bottom edge of the screen", &[("top", "Top"), ("bottom", "Bottom")]),
            ch(
                "bar.style",
                "Style",
                "One island in the middle, a pill on each side, or a full band",
                &[("island", "Island"), ("dynamic", "Dynamic"), ("flat", "Flat")],
            ),
            sl("bar.height", "Size", "How tall the bar is", 28.0, 56.0, 1.0, true),
            sl("bar.opacity", "Transparency", "Lower is more see-through", 0.2, 1.0, 0.02, false),
            sw("bar.autohide", "Auto-hide", "The bar slides away; touch the screen edge to bring it back"),
            sec("Workspaces", "On driftwm the bookmarks are the workspaces"),
            sl("bar.workspaces", "How many", "Workspaces always shown", 1.0, 10.0, 1.0, true),
            ch("bar.ws_style", "Style", "", &[("dots", "Dots"), ("numbers", "Numbers")]),
            sec("Clock", ""),
            sw("bar.seconds", "Seconds", "The island shows seconds (redraws every second)"),
            sw("bar.date", "Date", "The day beside the time"),
        ],
    },
    Page {
        icon: "magic_button",
        name: "Island",
        sub: "What the dynamic island grows into",
        rows: &[
            sw("island.dynamic", "Dynamic island", "Off: only the clock (a click still opens Today)"),
            sw("island.media", "Player", "Point at the island while music plays and the player grows out of it"),
            sw("island.volume", "Volume and brightness", "Changes show in the island with a wave you can drag"),
            sw("island.workspaces", "Workspaces", "Which workspace you switched to"),
            sw("island.notifications", "Notifications", "New notifications come out of the island"),
            sw("island.charger", "Charger", "Plugged in or out, with the battery level"),
            sw("island.weather", "Weather", "The weather beside the time"),
            sw("island.wave", "Little visualizer", "Four moving bars beside the time while music plays"),
        ],
    },
    Page {
        icon: "widgets",
        name: "Widgets",
        sub: "The desktop and the visualizer",
        rows: &[
            sw("widgets.enabled", "Desktop widgets", ""),
            sl("widgets.fps", "Frame rate", "Moving parts (a turning cover, waves) at most this often", 5.0, 60.0, 1.0, true),
            sw("widgets.pause_covered", "Pause under windows", "Motion stops while windows cover the desktop"),
            Row { key: "", title: "Widgets", desc: "Move them with Edit mode (SUPER + ALT + W)", ctl: Ctl::Button("Edit mode", || Act::EditMode) },
            sw("widgets.clock.enabled", "Clock", ""),
            ch("widgets.clock.style", "Clock style", "", &[("expressive", "Expressive"), ("stacked", "Stacked")]),
            sw("widgets.user.enabled", "User", "Hi, name! and the date"),
            sw("widgets.trio.enabled", "CPU · GPU · RAM", "In soft shapes"),
            sw("widgets.focus.enabled", "Focus timer", "Focus and break, with a wavy sun"),
            sw("widgets.player.enabled", "Player", "The cover in your shape"),
            sw("widgets.weather.enabled", "Weather", ""),
            sw("widgets.calendar.enabled", "Calendar", ""),
            sec("Dock", "Pinned apps and the open ones; right click an icon to pin or unpin it"),
            sw("dock.enabled", "Dock", ""),
            sw("dock.running", "Open apps", "Show apps that are open but not pinned"),
            sl("dock.size", "Icon size", "", 28.0, 72.0, 2.0, true),
            sec("Visualizer", "Uses cava; it runs only while music plays"),
            sw("widgets.visualizer.enabled", "Visualizer", ""),
            ch(
                "widgets.visualizer.style",
                "Style",
                "",
                &[("bars", "Bars"), ("mirror", "Mirror"), ("wave", "Wave"), ("dots", "Dots")],
            ),
            ch(
                "widgets.visualizer.color",
                "Color",
                "",
                &[("accent", "Theme"), ("gradient", "Gradient"), ("rainbow", "Rainbow"), ("mono", "White")],
            ),
            sl("widgets.visualizer.bars", "Bars", "", 16.0, 128.0, 1.0, true),
            sl("widgets.visualizer.height", "Height", "", 40.0, 220.0, 2.0, true),
        ],
    },
    Page {
        icon: "wallpaper",
        name: "Wallpaper",
        sub: "The picture and its colors",
        rows: &[
            Row { key: "", title: "Wallpapers", desc: "Pick one; the whole shell takes its colors", ctl: Ctl::Button("Open picker", || Act::OpenPicker) },
            Row { key: "", title: "Random", desc: "Another picture from your folder", ctl: Ctl::Button("Shuffle", || Act::WallShuffle) },
            sw("wallpaper.enabled", "Draw the wallpaper", "Off when swww, hyprpaper or driftwm's background shows it"),
            ch("wallpaper.transition", "Transition", "", &[("fade", "Fade"), ("none", "None")]),
        ],
    },
    Page {
        icon: "notifications",
        name: "Notifications",
        sub: "Pop-ups and the launcher",
        rows: &[
            sl("notifications.timeout", "Time on screen", "Seconds a pop-up stays", 2.0, 30.0, 1.0, true),
            sl("notifications.history", "History", "How many are kept in the control center", 5.0, 200.0, 5.0, true),
            Row { key: "", title: "Do Not Disturb", desc: "No pop-ups; they still go to the list", ctl: Ctl::Button("Toggle", || Act::Dnd) },
            Row { key: "", title: "Test", desc: "Send a notification", ctl: Ctl::Button("Send", || Act::NotifTest) },
            sec("Launcher", "ALT + Space"),
            sl("launcher.rows", "Rows", "", 3.0, 12.0, 1.0, true),
            sl("launcher.width", "Width", "", 400.0, 900.0, 10.0, true),
            ch("launcher.position", "Position", "", &[("top", "Top"), ("center", "Center"), ("bottom", "Bottom")]),
        ],
    },
    Page {
        icon: "tune",
        name: "System",
        sub: "Weather, performance and the computer",
        rows: &[
            sw("weather.enabled", "Weather", "From wttr.in every half hour"),
            ch("weather.units", "Units", "", &[("c", "Celsius"), ("f", "Fahrenheit")]),
            sec("Performance", ""),
            sw("performance.gaming", "Gaming mode", "No motion, no blur, no visualizer"),
            sw("performance.trim_memory", "Give memory back", "Return freed memory to the system after panels close"),
            sec("Power", ""),
            Row { key: "", title: "Session", desc: "Lock, sleep, log out, restart, shut down", ctl: Ctl::Button("Open", || Act::TogglePower) },
        ],
    },
    Page {
        icon: "info",
        name: "About",
        sub: "DarkShadow's Shell in Rust",
        rows: &[
            Row { key: "about.version", title: "Version", desc: "", ctl: Ctl::Info },
            Row { key: "about.wm", title: "Window manager", desc: "", ctl: Ctl::Info },
            Row { key: "about.memory", title: "Memory", desc: "What the shell itself uses right now", ctl: Ctl::Info },
            Row { key: "", title: "Config file", desc: "~/.config/dshell/config.toml (every option, with comments)", ctl: Ctl::Button("Open", || Act::OpenConfigFile) },
            Row { key: "", title: "Reload", desc: "Read the config file again", ctl: Ctl::Button("Reload", || Act::Reload) },
            sec("Credits", "Design: DarkShadow's Shell by shadowww-dev (MIT). Font: Google Sans Flex. Icons: Material Symbols."),
        ],
    },
];

pub fn find_row(key: &str) -> Option<&'static Row> {
    PAGES.iter().flat_map(|p| p.rows.iter()).find(|r| r.key == key)
}

fn get_path<'a>(v: &'a toml::Value, key: &str) -> Option<&'a toml::Value> {
    key.split('.').try_fold(v, |v, k| v.get(k))
}

fn set_path(v: &mut toml::Value, key: &str, val: toml::Value) {
    let parts: Vec<&str> = key.split('.').collect();
    let mut cur = v;
    for (i, p) in parts.iter().enumerate() {
        let Some(t) = cur.as_table_mut() else { return };
        if i + 1 == parts.len() {
            t.insert(p.to_string(), val);
            return;
        }
        cur = t.entry(p.to_string()).or_insert(toml::Value::Table(Default::default()));
    }
}

/// The current value of a key, from the running config.
pub fn value(cfg: &Config, key: &str) -> Option<toml::Value> {
    let v = toml::Value::try_from(cfg).ok()?;
    get_path(&v, key).cloned()
}

/// What decides which surfaces exist and their size: a change here rebuilds them.
fn layout_sig(c: &Config) -> String {
    let w = &c.widgets;
    let wsig: Vec<String> = [&w.clock, &w.user, &w.trio, &w.focus, &w.player, &w.weather, &w.calendar]
        .iter()
        .map(|x| format!("{}{}{}{}{}", x.enabled, x.anchor, x.x, x.y, x.scale))
        .collect();
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        c.bar.enabled,
        c.bar.position,
        c.bar.height,
        c.bar.autohide,
        c.bar.output,
        w.enabled,
        wsig.join(","),
        w.visualizer.enabled,
        w.visualizer.height,
        w.fps,
        c.wallpaper.enabled,
        c.dock.enabled,
        c.dock.size
    )
}

/// Set one option: apply it now, write it to the file a moment later.
pub fn set(app: &mut App, key: &str, val: toml::Value) {
    let Ok(mut root) = toml::Value::try_from(&app.cfg) else { return };
    if get_path(&root, key) == Some(&val) {
        return;
    }
    set_path(&mut root, key, val.clone());
    let Ok(new) = root.try_into::<Config>() else { return };
    let rebuild = layout_sig(&app.cfg) != layout_sig(&new);
    let notif_was = app.cfg.notifications.history;
    app.cfg = new;
    app.theme = make_theme(&app.cfg, &app.st);
    if rebuild {
        crate::shell::build(app);
    }
    if notif_was > app.cfg.notifications.history {
        let n = app.cfg.notifications.history as usize;
        app.st.notifs.list.truncate(n);
    }
    crate::svc::viz::update_wanted(app);
    crate::svc::sysmon::update_wanted(app);
    crate::shell::restart_clock(app);
    if key.starts_with("weather.") {
        app.svc.weather_at = None;
        crate::svc::weather::fetch(app);
    }
    app.mark(topic::ALL);
    app.ui.settings.pending.retain(|(k, _)| k != key);
    app.ui.settings.pending.push((key.to_string(), val));
    schedule_write(app);
}

fn schedule_write(app: &mut App) {
    if app.ui.settings.write_armed {
        return;
    }
    app.ui.settings.write_armed = true;
    let _ = app.lh.insert_source(Timer::from_duration(Duration::from_millis(500)), |_, _, app: &mut App| {
        app.ui.settings.write_armed = false;
        let keys = std::mem::take(&mut app.ui.settings.pending);
        write_keys(app, &keys);
        TimeoutAction::Drop
    });
}

/// Write some keys into the user's config.toml, keeping everything else in it.
pub fn write_keys(app: &mut App, keys: &[(String, toml::Value)]) {
    if keys.is_empty() {
        return;
    }
    let p = crate::config::path();
    let text = std::fs::read_to_string(&p).unwrap_or_default();
    let mut doc: toml::Value = toml::from_str::<toml::Table>(&text).map(toml::Value::Table).unwrap_or(toml::Value::Table(Default::default()));
    for (k, v) in keys {
        set_path(&mut doc, k, v.clone());
    }
    let Ok(out) = toml::to_string(&doc) else { return };
    let _ = std::fs::create_dir_all(p.parent().unwrap());
    // our own write: the file watcher must not reload (and rebuild) for it
    app.svc.reload_armed = true;
    let tmp = p.with_extension("toml.tmp");
    if std::fs::write(&tmp, out).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
    let _ = app.lh.insert_source(Timer::from_duration(Duration::from_millis(400)), |_, _, app: &mut App| {
        app.svc.reload_armed = false;
        TimeoutAction::Drop
    });
}

/// A slider moved to `f` (0..1).
pub fn slide(app: &mut App, key: &str, f: f32) {
    let Some(Row { ctl: Ctl::Slider { min, max, step, int }, .. }) = find_row(key) else { return };
    let raw = min + (max - min) * f as f64;
    let v = ((raw - min) / step).round() * step + min;
    let v = v.clamp(*min, *max);
    let val = if *int { toml::Value::Integer(v.round() as i64) } else { toml::Value::Float((v * 1000.0).round() / 1000.0) };
    set(app, key, val);
}

pub fn toggle(app: &mut App, key: &str) {
    let cur = value(&app.cfg, key).and_then(|v| v.as_bool()).unwrap_or(false);
    set(app, key, toml::Value::Boolean(!cur));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_exists_in_the_config() {
        let c = Config::default();
        for p in PAGES {
            for r in p.rows {
                if r.key.is_empty() || r.key.starts_with("about.") {
                    continue;
                }
                assert!(value(&c, r.key).is_some(), "settings key {} is not in Config", r.key);
            }
        }
    }

    #[test]
    fn values_round_trip_into_the_config() {
        let mut root = toml::Value::try_from(Config::default()).unwrap();
        set_path(&mut root, "bar.height", toml::Value::Integer(44));
        set_path(&mut root, "theme.panel_alpha", toml::Value::Float(0.5));
        set_path(&mut root, "widgets.visualizer.style", toml::Value::String("wave".into()));
        let c: Config = root.try_into().unwrap();
        assert_eq!(c.bar.height, 44);
        assert!((c.theme.panel_alpha - 0.5).abs() < 1e-6);
        assert_eq!(c.widgets.visualizer.style, "wave");
    }
}
