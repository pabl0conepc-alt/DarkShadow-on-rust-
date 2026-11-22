//! driftwm: an infinite canvas without workspaces. Its bookmarks play the part of workspaces (the same thing
//! its ext-workspace export does); `Subscribe` pushes a whole-state snapshot on every change.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use smithay_client_toolkit::reexports::calloop::{
    Interest, Mode, PostAction,
    generic::Generic,
    timer::{TimeoutAction, Timer},
};

use crate::app::App;
use crate::state::{WmKind, Ws, topic};

#[derive(Default)]
pub struct Drift {
    pub on: bool,
    bookmarks: Vec<(String, f64, f64)>,
    active_bm: Option<String>,
    pending: Option<String>,
    last: Option<Instant>,
    armed: bool,
}

pub fn socket() -> PathBuf {
    if let Some(p) = std::env::var_os("DRIFTWM_SOCKET") {
        return PathBuf::from(p);
    }
    let disp = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
    crate::config::runtime_dir().join("driftwm").join(format!("ipc-{disp}.sock"))
}

pub fn request(req: &Value) -> Option<Value> {
    let mut s = UnixStream::connect(socket()).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(500))).ok()?;
    s.write_all(format!("{req}\n").as_bytes()).ok()?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).ok()?;
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    v.get("Ok").cloned()
}

pub fn action(a: &str) {
    let _ = request(&json!({ "Action": a }));
}

pub fn start(app: &mut App) {
    // a socket file left by a driftwm that died does not count: it has to answer
    if UnixStream::connect(socket()).is_err() {
        return;
    }
    app.svc.drift.on = true;
    app.st.wm.kind = WmKind::Drift;
    connect(app);
}

fn natural(s: &str) -> (u64, String) {
    (s.parse::<u64>().unwrap_or(u64::MAX), s.to_string())
}

fn refresh_bookmarks(app: &mut App) {
    if let Some(v) = request(&json!({ "Bookmark": {} }))
        && let Some(map) = v.get("Bookmarks").and_then(Value::as_object)
    {
        let mut list: Vec<(String, f64, f64)> = map
            .iter()
            .filter_map(|(k, p)| {
                let a = p.as_array()?;
                Some((k.clone(), a.first()?.as_f64()?, a.get(1)?.as_f64()?))
            })
            .collect();
        list.sort_by_key(|a| natural(&a.0));
        app.svc.drift.bookmarks = list;
    }
}

fn connect(app: &mut App) {
    let Ok(mut s) = UnixStream::connect(socket()) else {
        retry(app);
        return;
    };
    if s.write_all(b"\"Subscribe\"\n").is_err() {
        retry(app);
        return;
    }
    let _ = s.set_nonblocking(true);
    refresh_bookmarks(app);
    let mut acc = Vec::<u8>::new();
    let _ = app.lh.insert_source(Generic::new(s, Interest::READ, Mode::Level), move |_, s, app| {
        let mut buf = [0u8; 32768];
        loop {
            match (&**s).read(&mut buf) {
                Ok(0) => {
                    retry(app);
                    return Ok(PostAction::Remove);
                }
                Ok(n) => {
                    acc.extend_from_slice(&buf[..n]);
                    while let Some(p) = acc.iter().position(|&b| b == b'\n') {
                        let line: Vec<u8> = acc.drain(..=p).collect();
                        let l = String::from_utf8_lossy(&line).trim().to_string();
                        if l.starts_with("{\"State\"") {
                            app.svc.drift.pending = Some(l);
                        }
                    }
                    if acc.len() > 4 << 20 {
                        acc.clear();
                    }
                    coalesce(app);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(PostAction::Continue),
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
    let _ = app.lh.insert_source(Timer::from_duration(Duration::from_secs(2)), |_, _, app: &mut App| {
        if socket().exists() {
            connect(app);
        } else {
            retry(app);
        }
        TimeoutAction::Drop
    });
}

/// While the camera glides driftwm sends a snapshot every frame; look at most 20 times a second.
fn coalesce(app: &mut App) {
    let d = &mut app.svc.drift;
    let now = Instant::now();
    let since = d.last.map(|l| now.saturating_duration_since(l)).unwrap_or(Duration::from_secs(1));
    if since >= Duration::from_millis(50) {
        process(app);
        return;
    }
    if d.armed {
        return;
    }
    d.armed = true;
    let _ = app.lh.insert_source(Timer::from_duration(Duration::from_millis(50) - since), |_, _, app: &mut App| {
        app.svc.drift.armed = false;
        process(app);
        TimeoutAction::Drop
    });
}

fn process(app: &mut App) {
    let Some(line) = app.svc.drift.pending.take() else { return };
    app.svc.drift.last = Some(Instant::now());
    let Ok(v) = serde_json::from_str::<Value>(&line) else { return };
    let st = &v["State"];
    let active_bm = st["active_bookmark"].as_str().map(str::to_string);
    if active_bm != app.svc.drift.active_bm || app.svc.drift.bookmarks.is_empty() {
        refresh_bookmarks(app);
    }
    let ws_changed = active_bm != app.svc.drift.active_bm && app.svc.drift.active_bm.is_some();
    app.svc.drift.active_bm = active_bm.clone();
    // the focused output's viewport
    let outs = st["outputs"].as_array().cloned().unwrap_or_default();
    let out = outs.iter().find(|o| o["active"].as_bool() == Some(true)).or(outs.first());
    let (vw, vh, cx, cy) = match out {
        Some(o) => {
            let z = o["zoom"].as_f64().unwrap_or(1.0).max(0.05);
            (
                o["size"][0].as_f64().unwrap_or(1920.0) / z,
                o["size"][1].as_f64().unwrap_or(1080.0) / z,
                o["camera"][0].as_f64().unwrap_or(0.0),
                o["camera"][1].as_f64().unwrap_or(0.0),
            )
        }
        None => (1920.0, 1080.0, 0.0, 0.0),
    };
    let wins = st["windows"].as_array().cloned().unwrap_or_default();
    let overlaps = |wx: f64, wy: f64, ww: f64, wh: f64, x: f64, y: f64| {
        (wx - x).abs() < (ww + vw) / 2.0 - 1.0 && (wy - y).abs() < (wh + vh) / 2.0 - 1.0
    };
    let live: Vec<&Value> = wins.iter().filter(|w| !w["is_widget"].as_bool().unwrap_or(false)).collect();
    let mut list = Vec::new();
    let mut active = 0;
    for (i, (name, bx, by)) in app.svc.drift.bookmarks.iter().enumerate() {
        let n = live
            .iter()
            .filter(|w| {
                overlaps(
                    w["position"][0].as_f64().unwrap_or(0.0),
                    w["position"][1].as_f64().unwrap_or(0.0),
                    w["size"][0].as_f64().unwrap_or(0.0),
                    w["size"][1].as_f64().unwrap_or(0.0),
                    *bx,
                    *by,
                )
            })
            .count();
        if Some(name) == active_bm.as_ref() {
            active = i as i32 + 1;
        }
        list.push(Ws { id: i as i32 + 1, name: name.clone(), windows: n as u32 });
    }
    let covered = live.iter().any(|w| {
        overlaps(
            w["position"][0].as_f64().unwrap_or(0.0),
            w["position"][1].as_f64().unwrap_or(0.0),
            w["size"][0].as_f64().unwrap_or(0.0),
            w["size"][1].as_f64().unwrap_or(0.0),
            cx,
            cy,
        )
    }) || st["fullscreen"].as_array().map(|a| !a.is_empty()).unwrap_or(false);
    let focused = wins.iter().find(|w| w["is_focused"].as_bool() == Some(true));
    let wm = &mut app.st.wm;
    let old_cov = wm.covered;
    wm.workspaces = list;
    wm.active = active;
    wm.covered = covered;
    wm.fullscreen = st["fullscreen"].as_array().map(|a| !a.is_empty()).unwrap_or(false);
    wm.title = focused.and_then(|w| w["title"].as_str()).unwrap_or("").to_string();
    wm.class = focused.and_then(|w| w["app_id"].as_str()).unwrap_or("").to_string();
    wm.layout = st["layout_short"].as_str().unwrap_or("").to_string();
    app.mark(topic::WM);
    if ws_changed {
        crate::actions::island_flash(app, crate::ui::IslandMode::Ws);
    }
    if old_cov != covered {
        crate::shell::covered_changed(app);
    }
}

/// Go to the n-th bookmark (1-based) or step through them.
pub fn goto(app: &mut App, n: i32) {
    if let Some((name, _, _)) = app.svc.drift.bookmarks.get((n - 1).max(0) as usize) {
        action(&format!("go-to-bookmark {name}"));
    }
}

pub fn step(app: &mut App, d: i32) {
    let len = app.svc.drift.bookmarks.len() as i32;
    if len == 0 {
        return;
    }
    let cur = app.st.wm.active.max(1);
    let next = (cur - 1 + d).rem_euclid(len) + 1;
    goto(app, next);
}
