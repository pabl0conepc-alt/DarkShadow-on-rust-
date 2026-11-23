//! Hyprland: the event socket (.socket2) tells us when something changed; the request socket answers what.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;
use smithay_client_toolkit::reexports::calloop::{
    Interest, Mode, PostAction,
    generic::Generic,
    timer::{TimeoutAction, Timer},
};

use crate::app::App;
use crate::state::{WmKind, Ws, topic};

#[derive(Default)]
pub struct Hypr {
    pub dir: Option<PathBuf>,
    refresh_armed: bool,
}

fn sock_dir() -> Option<PathBuf> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let rt = crate::config::runtime_dir().join("hypr").join(&sig);
    if rt.exists() {
        return Some(rt);
    }
    let old = PathBuf::from("/tmp/hypr").join(&sig);
    old.exists().then_some(old)
}

/// One request on the command socket ("j/workspaces", "dispatch workspace 3" ...).
pub fn request(app: &App, cmd: &str) -> Option<String> {
    let dir = app.svc.hypr.dir.as_ref()?;
    let mut s = UnixStream::connect(dir.join(".socket.sock")).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(400))).ok()?;
    s.set_write_timeout(Some(Duration::from_millis(400))).ok()?;
    s.write_all(cmd.as_bytes()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    Some(out)
}

pub fn dispatch(app: &App, d: &str) {
    let _ = request(app, &format!("dispatch {d}"));
}

pub fn start(app: &mut App) {
    let Some(dir) = sock_dir() else { return };
    app.svc.hypr.dir = Some(dir);
    app.st.wm.kind = WmKind::Hyprland;
    connect_events(app);
    refresh(app);
}

fn connect_events(app: &mut App) {
    let Some(dir) = app.svc.hypr.dir.clone() else {
        return;
    };
    let Ok(stream) = UnixStream::connect(dir.join(".socket2.sock")) else {
        retry(app);
        return;
    };
    let _ = stream.set_nonblocking(true);
    let mut acc = Vec::<u8>::new();
    let src = Generic::new(stream, Interest::READ, Mode::Level);
    let _ = app.lh.insert_source(src, move |_, s, app| {
        let mut buf = [0u8; 8192];
        loop {
            match (&**s).read(&mut buf) {
                Ok(0) => {
                    retry(app);
                    return Ok(PostAction::Remove);
                }
                Ok(n) => {
                    acc.extend_from_slice(&buf[..n]);
                    let mut want = false;
                    while let Some(p) = acc.iter().position(|&b| b == b'\n') {
                        let line: Vec<u8> = acc.drain(..=p).collect();
                        let l = String::from_utf8_lossy(&line);
                        let ev = l.split(">>").next().unwrap_or("");
                        match ev {
                            "workspace" | "workspacev2" | "focusedmon" | "focusedmonv2"
                            | "activewindow" | "activewindowv2" | "openwindow" | "closewindow"
                            | "movewindow" | "movewindowv2" | "createworkspace"
                            | "createworkspacev2" | "destroyworkspace" | "destroyworkspacev2"
                            | "fullscreen" | "windowtitle" | "windowtitlev2"
                            | "changefloatingmode" | "moveworkspace" | "moveworkspacev2"
                            | "renameworkspace" => want = true,
                            "activelayout" => {
                                if let Some(v) = l.trim_end().split(">>").nth(1)
                                    && let Some(layout) = v.rsplit(',').next()
                                {
                                    app.st.wm.layout = layout.to_string();
                                    app.mark(topic::WM);
                                }
                            }
                            _ => {}
                        }
                    }
                    if want {
                        arm_refresh(app);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    return Ok(PostAction::Continue);
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    retry(app);
                    return Ok(PostAction::Remove);
                }
            }
        }
    });
}

fn retry(app: &mut App) {
    let _ = app.lh.insert_source(
        Timer::from_duration(Duration::from_secs(2)),
        |_, _, app: &mut App| {
            if sock_dir().is_some() {
                app.svc.hypr.dir = sock_dir();
                connect_events(app);
                refresh(app);
            }
            TimeoutAction::Drop
        },
    );
}

/// Events come in bursts (a window opening sends five): answer them once.
fn arm_refresh(app: &mut App) {
    if app.svc.hypr.refresh_armed {
        return;
    }
    app.svc.hypr.refresh_armed = true;
    let _ = app.lh.insert_source(
        Timer::from_duration(Duration::from_millis(12)),
        |_, _, app: &mut App| {
            app.svc.hypr.refresh_armed = false;
            refresh(app);
            TimeoutAction::Drop
        },
    );
}

pub fn refresh(app: &mut App) {
    let wss: Option<Value> =
        request(app, "j/workspaces").and_then(|s| serde_json::from_str(&s).ok());
    let aw: Option<Value> =
        request(app, "j/activeworkspace").and_then(|s| serde_json::from_str(&s).ok());
    let win: Option<Value> =
        request(app, "j/activewindow").and_then(|s| serde_json::from_str(&s).ok());
    let clients: Option<Value> = if app.cfg.dock.enabled {
        request(app, "j/clients").and_then(|s| serde_json::from_str(&s).ok())
    } else {
        None
    };
    let mut list = Vec::new();
    if let Some(Value::Array(a)) = wss {
        for w in a {
            let id = w["id"].as_i64().unwrap_or(0) as i32;
            if id <= 0 {
                continue; // special workspaces
            }
            list.push(Ws {
                id,
                name: w["name"].as_str().unwrap_or("").to_string(),
                windows: w["windows"].as_u64().unwrap_or(0) as u32,
            });
        }
    }
    list.sort_by_key(|w| w.id);
    let wm = &mut app.st.wm;
    let old_active = wm.active;
    let old_covered = wm.covered;
    wm.workspaces = list;
    if let Some(a) = &aw {
        wm.active = a["id"].as_i64().unwrap_or(1) as i32;
        wm.covered = a["windows"].as_u64().unwrap_or(0) > 0;
        wm.fullscreen = a["hasfullscreen"].as_bool().unwrap_or(false);
    }
    match &win {
        Some(w) if w.get("class").is_some() => {
            wm.title = w["title"].as_str().unwrap_or("").to_string();
            wm.class = w["class"].as_str().unwrap_or("").to_string();
        }
        _ => {
            wm.title.clear();
            wm.class.clear();
        }
    }
    let mut apps: Vec<(String, u32, bool)> = Vec::new();
    if let Some(Value::Array(cl)) = clients {
        let focused = app.st.wm.class.clone();
        for c in cl {
            let class = c["class"].as_str().unwrap_or("").to_string();
            if class.is_empty() || c["workspace"]["id"].as_i64().unwrap_or(1) < 0 {
                continue;
            }
            match apps.iter_mut().find(|a| a.0 == class) {
                Some(a) => a.1 += 1,
                None => {
                    let f = class == focused;
                    apps.push((class, 1, f));
                }
            }
        }
    }
    app.st.wm.apps = apps;
    let ws_changed = old_active != app.st.wm.active && old_active != 0;
    let cov_changed = old_covered != app.st.wm.covered;
    app.mark(topic::WM);
    if ws_changed {
        crate::actions::island_flash(app, crate::ui::IslandMode::Ws);
    }
    if cov_changed {
        crate::shell::covered_changed(app);
    }
}
