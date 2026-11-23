//! VEYRON WM: a tiling compositor written in Rust with a line-delimited JSON control socket
//! (`$VEYRON_SOCKET`, else `$XDG_RUNTIME_DIR/veyron.sock`). `subscribe` pushes the state of the active
//! workspace on every change (workspace, focus, windows, layout, theme; never geometry), and `overview`
//! lists every workspace with its windows, which gives the bar its dots.
//!
//! Veyron runs nested inside another session for now, so a Hyprland or KDE outside can leave its own
//! variables behind: the shell only takes Veyron for its window manager when Veyron is the one serving
//! our `WAYLAND_DISPLAY`.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};
use smithay_client_toolkit::reexports::calloop::{
    Interest, Mode, PostAction,
    generic::Generic,
    timer::{TimeoutAction, Timer},
};

use crate::app::App;
use crate::state::{WmKind, Ws, topic};

#[derive(Default)]
pub struct Veyron {
    pub on: bool,
    /// window id -> app id, learned whenever a window shows up on the active workspace
    /// (the overview only lists ids)
    known: HashMap<u64, String>,
    /// the app ids on each workspace (overview order), for the dock
    ws_apps: Vec<Vec<String>>,
    /// the overview was read at least once
    seen_overview: bool,
    focused: Option<u64>,
    /// the active workspace's windows: (id, app id)
    here: Vec<(u64, String)>,
    active_ws: Option<String>,
}

/// The control socket found at start (see `serves_us`).
static SOCKET: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

fn default_socket() -> PathBuf {
    if let Some(p) = std::env::var_os("VEYRON_SOCKET").filter(|v| !v.is_empty()) {
        return PathBuf::from(p);
    }
    crate::config::runtime_dir().join("veyron.sock")
}

pub fn socket() -> PathBuf {
    SOCKET.get().cloned().unwrap_or_else(default_socket)
}

/// `$VEYRON_SOCKET` or the default first, then any other listening socket with "veyron" in its name
/// (`veyron --socket PATH` does not tell its children where it is).
fn candidates() -> Vec<PathBuf> {
    let mut out = vec![default_socket()];
    if let Ok(table) = std::fs::read_to_string("/proc/net/unix") {
        for l in table.lines().skip(1) {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() >= 8 && f[3] == "00010000" && f[7].ends_with(".sock") {
                let p = PathBuf::from(f[7]);
                if p.file_name().is_some_and(|n| n.to_string_lossy().contains("veyron")) && !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// One request, one answer (a JSON object with a "response" tag).
pub fn request(req: &Value) -> Option<Value> {
    let mut s = UnixStream::connect(socket()).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(500))).ok()?;
    s.set_write_timeout(Some(Duration::from_millis(500))).ok()?;
    s.write_all(format!("{req}\n").as_bytes()).ok()?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).ok()?;
    serde_json::from_str(line.trim()).ok()
}

/// Run an action in Veyron's keybinding grammar ("workspace 2", "focus-cycle next", "quit").
pub fn action(a: &str) -> bool {
    request(&json!({ "request": "action", "action": a })).is_some_and(|v| v["response"] == "ok")
}

fn alive_at(path: &std::path::Path) -> bool {
    let ping = || -> Option<bool> {
        let mut s = UnixStream::connect(path).ok()?;
        s.set_read_timeout(Some(Duration::from_millis(300))).ok()?;
        s.write_all(b"{\"request\":\"ping\"}\n").ok()?;
        let mut line = String::new();
        BufReader::new(s).read_line(&mut line).ok()?;
        Some(line.contains("pong"))
    };
    ping().unwrap_or(false)
}

/// The path of the Wayland socket we draw on.
fn display_path() -> Option<PathBuf> {
    let d = std::env::var_os("WAYLAND_DISPLAY").unwrap_or_else(|| "wayland-0".into());
    let p = PathBuf::from(&d);
    Some(if p.is_absolute() { p } else { crate::config::runtime_dir().join(p) })
}

/// The process on the other end of a unix socket.
fn peer_pid(s: &UnixStream) -> Option<i32> {
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: SO_PEERCRED writes one `ucred` into the buffer we own; len holds its size
    let r = unsafe {
        libc::getsockopt(s.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&mut cred as *mut libc::ucred).cast(), &mut len)
    };
    (r == 0 && cred.pid > 0).then_some(cred.pid)
}

/// The inode of the listening unix socket bound at `path` (from /proc/net/unix).
fn socket_inode(path: &std::path::Path) -> Option<u64> {
    let want = path.to_str()?;
    let table = std::fs::read_to_string("/proc/net/unix").ok()?;
    table.lines().skip(1).find_map(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        // Num RefCount Protocol Flags Type St Inode Path; flags 00010000 = listening
        (f.len() >= 8 && f[7] == want && f[3] == "00010000").then(|| f[6].parse().ok()).flatten()
    })
}

/// Whether the Veyron answering on the control socket is the compositor serving our Wayland display:
/// its process holds the listening socket. `None` when it cannot be told (no /proc, another user).
fn owns_display(path: &std::path::Path) -> Option<bool> {
    let s = UnixStream::connect(path).ok()?;
    let pid = peer_pid(&s)?;
    let inode = socket_inode(&display_path()?)?;
    let want = format!("socket:[{inode}]");
    let fds = std::fs::read_dir(format!("/proc/{pid}/fd")).ok()?;
    Some(fds.flatten().any(|e| std::fs::read_link(e.path()).is_ok_and(|l| l.to_str() == Some(want.as_str()))))
}

/// Veyron is ours for sure (it serves our display): it wins over a Hyprland or KDE it is nested in.
pub fn serves_us() -> bool {
    let found = candidates().into_iter().find(|p| p.exists() && alive_at(p) && owns_display(p) == Some(true));
    found.map(|p| SOCKET.set(p)).is_some()
}

/// No other window manager was found: a Veyron that answers is good enough unless it is known to serve
/// another display.
pub fn plausible() -> bool {
    let p = default_socket();
    p.exists() && alive_at(&p) && owns_display(&p) != Some(false)
}

pub fn start(app: &mut App) {
    app.svc.veyron.on = true;
    app.st.wm.kind = WmKind::Veyron;
    fetch_theme(app);
    app.theme = crate::app::make_theme(&app.cfg, &app.st);
    connect(app);
}

fn connect(app: &mut App) {
    let Ok(mut s) = UnixStream::connect(socket()) else {
        retry(app);
        return;
    };
    if s.write_all(b"{\"request\":\"subscribe\"}\n").is_err() {
        retry(app);
        return;
    }
    let _ = s.set_nonblocking(true);
    let mut acc = Vec::<u8>::new();
    let _ = app.lh.insert_source(Generic::new(s, Interest::READ, Mode::Level), move |_, s, app| {
        let mut buf = [0u8; 16384];
        let mut last: Option<String> = None;
        let mut want_overview = false;
        let mut want_theme = false;
        let result = loop {
            match (&**s).read(&mut buf) {
                Ok(0) => break Err(()),
                Ok(n) => {
                    acc.extend_from_slice(&buf[..n]);
                    while let Some(p) = acc.iter().position(|&b| b == b'\n') {
                        let line: Vec<u8> = acc.drain(..=p).collect();
                        let l = String::from_utf8_lossy(&line).trim().to_string();
                        if l.contains("\"response\":\"event\"") {
                            // focus alone does not change which workspaces have windows
                            want_overview |= l.contains("\"workspace\"") || l.contains("\"windows\"");
                            want_theme |= l.contains("\"theme\"");
                            last = Some(l);
                        }
                    }
                    if acc.len() > 1 << 20 {
                        acc.clear();
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break Err(()),
            }
        };
        // a burst of events only needs the newest state
        if want_theme && fetch_theme(app) && app.cfg.theme.scheme == "veyron" {
            app.theme = crate::app::make_theme(&app.cfg, &app.st);
            app.mark(crate::state::topic::ALL);
        }
        if let Some(l) = last {
            process(app, &l, want_overview);
        }
        match result {
            Ok(()) => Ok(PostAction::Continue),
            Err(()) => {
                retry(app);
                Ok(PostAction::Remove)
            }
        }
    });
}

fn retry(app: &mut App) {
    let _ = app.lh.insert_source(Timer::from_duration(Duration::from_secs(2)), |_, _, app: &mut App| {
        if socket().exists() {
            connect(app);
        } else {
            retry(app);
        }
        TimeoutAction::Drop
    });
}

fn process(app: &mut App, line: &str, want_overview: bool) {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return };
    let st = &v["state"];
    let vy = &mut app.svc.veyron;
    let wins = st["windows"].as_array().cloned().unwrap_or_default();
    vy.here.clear();
    for w in &wins {
        let (Some(id), app_id) = (w["id"].as_u64(), w["app_id"].as_str().unwrap_or("")) else { continue };
        if !app_id.is_empty() {
            vy.known.insert(id, app_id.to_string());
        }
        vy.here.push((id, app_id.to_string()));
    }
    vy.focused = st["focused"].as_u64();
    let ws_name = st["workspace"].as_str().unwrap_or("").to_string();
    let ws_changed = vy.active_ws.as_ref().is_some_and(|w| *w != ws_name);
    vy.active_ws = Some(ws_name);
    let index = st["workspace_index"].as_u64().unwrap_or(0) as usize;
    let count = st["workspace_count"].as_u64().unwrap_or(1).max(1) as usize;

    // every workspace and its windows
    let mut list: Vec<Ws> = Vec::new();
    if (want_overview || !app.svc.veyron.seen_overview)
        && let Some(o) = request(&json!({ "request": "overview" })).filter(|o| o["response"] == "overview")
    {
        let mut tiles: Vec<&Value> = o["tiles"].as_array().map(|t| t.iter().collect()).unwrap_or_default();
        tiles.sort_by_key(|t| t["index"].as_u64().unwrap_or(0));
        let vy = &mut app.svc.veyron;
        vy.seen_overview = true;
        let ids: Vec<Vec<u64>> = tiles
            .iter()
            .map(|t| t["windows"].as_array().map(|w| w.iter().filter_map(|w| w["id"].as_u64()).collect()).unwrap_or_default())
            .collect();
        // forget windows that are gone
        let alive: std::collections::HashSet<u64> = ids.iter().flatten().copied().collect();
        vy.known.retain(|id, _| alive.contains(id));
        // windows never seen on the active workspace (the shell started late, or a rule sent them
        // away): the session capture tells which apps each workspace holds
        let names: Vec<String> = tiles.iter().map(|t| t["workspace"].as_str().unwrap_or("").to_string()).collect();
        let unknown = ids.iter().flatten().any(|id| !vy.known.contains_key(id));
        let by_name = if unknown { session_apps() } else { HashMap::new() };
        vy.ws_apps = ids
            .iter()
            .zip(&names)
            .map(|(ids, name)| match by_name.get(name) {
                Some(apps) if ids.iter().any(|id| !vy.known.contains_key(id)) => apps.clone(),
                _ => ids.iter().filter_map(|id| vy.known.get(id).cloned()).collect(),
            })
            .collect();
        list = tiles
            .iter()
            .enumerate()
            .map(|(i, t)| Ws {
                id: i as i32 + 1,
                name: t["workspace"].as_str().unwrap_or("").to_string(),
                windows: t["windows"].as_array().map(|w| w.len()).unwrap_or(0) as u32,
            })
            .collect();
    }
    let vy = &mut app.svc.veyron;
    if list.is_empty() {
        // no overview: keep the last list, fixing the active workspace's count
        list = app.st.wm.workspaces.clone();
        list.resize_with(count, Ws::default);
        for (i, w) in list.iter_mut().enumerate() {
            w.id = i as i32 + 1;
        }
        if let Some(w) = list.get_mut(index) {
            w.windows = vy.here.len() as u32;
        }
        vy.ws_apps.resize_with(count, Vec::new);
    }

    // the active workspace is known exactly
    if let Some(ws) = vy.ws_apps.get_mut(index) {
        *ws = vy.here.iter().map(|w| w.1.clone()).filter(|a| !a.is_empty()).collect();
    }
    // open apps on every workspace, grouped
    let mut apps: Vec<(String, u32, bool)> = Vec::new();
    for a in vy.ws_apps.iter().flatten() {
        match apps.iter_mut().find(|x| x.0 == *a) {
            Some(x) => x.1 += 1,
            None => apps.push((a.clone(), 1, false)),
        }
    }
    if let Some(f) = vy.here.iter().find(|w| Some(w.0) == vy.focused)
        && let Some(x) = apps.iter_mut().find(|x| x.0 == f.1)
    {
        x.2 = true;
    }

    let focused = wins.iter().find(|w| w["id"].as_u64().is_some() && w["id"].as_u64() == vy.focused);
    let fullscreen = wins.iter().any(|w| w["fullscreen"].as_bool() == Some(true));
    // a tiled, maximized or fullscreen window fills the desktop; small floating ones leave it showing
    let covered = wins.iter().any(|w| {
        w["floating"].as_bool() != Some(true)
            || w["maximized"].as_bool() == Some(true)
            || w["fullscreen"].as_bool() == Some(true)
            || w["rect"][2].as_i64().unwrap_or(0) * w["rect"][3].as_i64().unwrap_or(0) > 1280 * 720
    });
    let wm = &mut app.st.wm;
    let old_cov = wm.covered;
    wm.apps = apps;
    wm.workspaces = list;
    wm.active = index as i32 + 1;
    wm.covered = covered;
    wm.fullscreen = fullscreen;
    wm.title = focused.and_then(|w| w["title"].as_str()).unwrap_or("").to_string();
    wm.class = focused.and_then(|w| w["app_id"].as_str()).unwrap_or("").to_string();
    wm.layout = st["layout"].as_str().unwrap_or("").to_string();
    app.mark(topic::WM);
    if ws_changed {
        crate::actions::island_flash(app, crate::ui::IslandMode::Ws);
    }
    if old_cov != covered {
        crate::shell::covered_changed(app);
    }
}

/// Veyron's Material You primary color; true when it changed.
fn fetch_theme(app: &mut App) -> bool {
    let c = request(&json!({ "request": "theme" }))
        .filter(|v| v["response"] == "theme")
        .and_then(|v| v["colors"]["primary"].as_str().and_then(crate::color::Rgba::hex));
    if c.is_none() || c == app.st.wm.accent {
        return false;
    }
    app.st.wm.accent = c;
    true
}

/// workspace name -> the app ids of its windows, from the session capture.
fn session_apps() -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    let Some(v) = request(&json!({ "request": "capture-session" })).filter(|v| v["response"] == "session") else { return out };
    let Some(t) = v["toml"].as_str().and_then(|t| t.parse::<toml::Table>().ok()) else { return out };
    for w in t.get("windows").and_then(|w| w.as_array()).into_iter().flatten() {
        let (Some(ws), Some(a)) = (w.get("workspace").and_then(|x| x.as_str()), w.get("app_id").and_then(|x| x.as_str())) else { continue };
        if !a.is_empty() {
            out.entry(ws.to_string()).or_default().push(a.to_string());
        }
    }
    out
}

pub fn goto(n: i32) {
    action(&format!("workspace {}", n.max(1)));
}

pub fn step(d: i32) {
    action(if d > 0 { "workspace next" } else { "workspace prev" });
}

/// The focused window and the active workspace's windows, asked right now.
/// (the focused window, the active workspace's windows as (id, app id))
type Now = (Option<u64>, Vec<(u64, String)>);

fn now_state() -> Option<Now> {
    let v = request(&json!({ "request": "state" })).filter(|v| v["response"] == "state")?;
    let wins = v["windows"]
        .as_array()?
        .iter()
        .filter_map(|w| Some((w["id"].as_u64()?, w["app_id"].as_str().unwrap_or("").to_string())))
        .collect();
    Some((v["focused"].as_u64(), wins))
}

/// Bring a window of `class` forward. Veyron has no "focus window <id>" action: go to its workspace,
/// then cycle the focus until it lands on one (a click on the focused app steps to its next window).
pub fn focus_app(app: &mut App, class: &str) {
    let vy = &app.svc.veyron;
    let here = vy.here.iter().any(|w| w.1 == class);
    if !here {
        let ws = vy.ws_apps.iter().position(|apps| apps.iter().any(|a| a == class));
        match ws {
            Some(i) => goto(i as i32 + 1),
            None => return,
        }
    }
    let Some((mut focused, wins)) = now_state() else { return };
    let mine = |f: Option<u64>| f.is_some_and(|f| wins.iter().any(|w| w.0 == f && w.1 == class));
    if mine(focused) && here {
        // already there: the next window of the same app, if there is one
        let start = focused;
        for _ in 0..wins.len() {
            if !action("focus-cycle next") {
                return;
            }
            focused = now_state().and_then(|s| s.0);
            if mine(focused) || focused == start {
                return;
            }
        }
        return;
    }
    for _ in 0..wins.len() {
        if mine(focused) || !action("focus-cycle next") {
            return;
        }
        focused = now_state().and_then(|s| s.0);
    }
}
