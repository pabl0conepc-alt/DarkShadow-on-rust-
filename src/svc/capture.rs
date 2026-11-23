//! Screenshots (grim + slurp) and screen recording (wf-recorder). The island shows the recording with its
//! time and a stop button; a notification shows the picture or the video when it is done.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};

use crate::app::App;
use crate::proc;
use crate::state::topic;

#[derive(Default)]
pub struct Capture {
    pub child: Option<std::process::Child>,
}

fn stamp() -> String {
    let t = crate::clock::now();
    format!("{}-{:02}-{:02}_{:02}-{:02}-{:02}", t.year, t.mon + 1, t.mday, t.hour, t.min, t.sec)
}

fn dir(kind: &str) -> PathBuf {
    let home = crate::config::home();
    let base = home.join(if kind == "video" { "Videos" } else { "Pictures" });
    let d = if kind == "video" { base.join("Recordings") } else { base.join("Screenshots") };
    let _ = std::fs::create_dir_all(&d);
    d
}

/// The name of the shell's screen for grim -o / wf-recorder -o.
fn main_output_name(app: &App) -> Option<String> {
    app.svc.main_output.as_ref().and_then(|o| app.outputs.info(o)).and_then(|i| i.name)
}

/// `region` | `full` | `window` (the focused window on Hyprland).
pub fn shot(app: &mut App, what: &str) {
    if !proc::which("grim") {
        crate::actions::system_notice(app, "Screenshot", "grim is not installed");
        return;
    }
    crate::actions::close_all(app);
    let file = dir("pictures").join(format!("Screenshot_{}.png", stamp()));
    let f = file.to_string_lossy().into_owned();
    let q = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let cmd = match what {
        "region" => {
            if !proc::which("slurp") {
                crate::actions::system_notice(app, "Screenshot", "slurp is not installed (needed to pick a region)");
                return;
            }
            format!("g=$(slurp -d 2>/dev/null) && grim -g \"$g\" {}", q(&f))
        }
        "window" if app.st.wm.kind == crate::state::WmKind::Hyprland => {
            let geo = crate::svc::hypr::request(app, "j/activewindow")
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| Some(format!("{},{} {}x{}", v["at"][0].as_i64()?, v["at"][1].as_i64()?, v["size"][0].as_i64()?, v["size"][1].as_i64()?)));
            match geo {
                Some(g) => format!("grim -g '{g}' {}", q(&f)),
                None => format!("grim {}", q(&f)),
            }
        }
        _ => match main_output_name(app) {
            Some(o) => format!("grim -o {} {}", q(&o), q(&f)),
            None => format!("grim {}", q(&f)),
        },
    };
    // a moment for the panels to fade before the picture is taken
    let _ = app.lh.insert_source(Timer::from_duration(Duration::from_millis(260)), move |_, _, app: &mut App| {
        let file = file.clone();
        proc::capture(app, &["sh", "-c", &cmd], move |app, _| {
            if !file.is_file() {
                return; // cancelled (Esc in slurp)
            }
            let p = file.to_string_lossy().into_owned();
            if proc::which("wl-copy") {
                proc::sh(&format!("wl-copy --type image/png < '{}'", p.replace('\'', "'\\''")));
            }
            let id = notice(app, "Screenshot saved", &format!("{} · copied", file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
            crate::svc::images::load_notif_icon(app, id, p);
        });
        TimeoutAction::Drop
    });
}

fn notice(app: &mut App, summary: &str, body: &str) -> u32 {
    crate::actions::system_notice(app, summary, body);
    app.st.notifs.list.first().map(|n| n.id).unwrap_or(0)
}

/// Start (or stop) a recording of a region, the screen, with or without sound.
pub fn record(app: &mut App, what: &str) {
    if app.st.rec.on || what == "stop" {
        stop(app);
        return;
    }
    if !proc::which("wf-recorder") {
        crate::actions::system_notice(app, "Recording", "wf-recorder is not installed");
        return;
    }
    crate::actions::close_all(app);
    let file = dir("video").join(format!("Recording_{}.mp4", stamp()));
    let mut args: Vec<String> = vec!["-f".into(), file.to_string_lossy().into_owned()];
    if what.contains("sound") {
        args.push("-a".into());
    }
    let region = what.contains("region");
    let out = main_output_name(app);
    let path = file.clone();
    let start = move |app: &mut App, geo: Option<String>| {
        let mut a = args.clone();
        match geo {
            Some(g) => {
                a.push("-g".into());
                a.push(g);
            }
            None => {
                if let Some(o) = &out {
                    a.push("-o".into());
                    a.push(o.clone());
                }
            }
        }
        let child = std::process::Command::new("wf-recorder")
            .args(&a)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        match child {
            Ok(c) => {
                app.svc.capture.child = Some(c);
                app.st.rec.on = true;
                app.st.rec.since = Some(Instant::now());
                app.st.rec.path = path.to_string_lossy().into_owned();
                app.mark(topic::REC);
                crate::shell::restart_clock(app);
                watch(app);
            }
            Err(e) => crate::actions::system_notice(app, "Recording", &format!("wf-recorder did not start: {e}")),
        }
    };
    if region {
        if !proc::which("slurp") {
            crate::actions::system_notice(app, "Recording", "slurp is not installed (needed to pick a region)");
            return;
        }
        proc::capture(app, &["slurp", "-d"], move |app, out| {
            let g = out.trim().to_string();
            if !g.is_empty() {
                start(app, Some(g));
            }
        });
    } else {
        start(app, None);
    }
}

/// A recorder that quit by itself (the screen went away, an error) still ends the recording.
fn watch(app: &mut App) {
    let _ = app.lh.insert_source(Timer::from_duration(Duration::from_secs(2)), |_, _, app: &mut App| {
        if !app.st.rec.on {
            return TimeoutAction::Drop;
        }
        let gone = match app.svc.capture.child.as_mut() {
            Some(c) => c.try_wait().map(|s| s.is_some()).unwrap_or(true),
            None => true,
        };
        if gone {
            finish(app);
            return TimeoutAction::Drop;
        }
        TimeoutAction::ToDuration(Duration::from_secs(2))
    });
}

pub fn stop(app: &mut App) {
    if let Some(c) = app.svc.capture.child.as_ref() {
        // SIGINT lets wf-recorder write the end of the file
        // SAFETY: signalling our own child process.
        unsafe {
            libc::kill(c.id() as i32, libc::SIGINT);
        }
    }
    let _ = app.lh.insert_source(Timer::from_duration(Duration::from_millis(400)), |_, _, app: &mut App| {
        if let Some(c) = app.svc.capture.child.as_mut()
            && c.try_wait().map(|s| s.is_none()).unwrap_or(false)
        {
            return TimeoutAction::ToDuration(Duration::from_millis(300));
        }
        finish(app);
        TimeoutAction::Drop
    });
}

fn finish(app: &mut App) {
    if let Some(mut c) = app.svc.capture.child.take() {
        let _ = c.wait();
    }
    if !app.st.rec.on {
        return;
    }
    app.st.rec.on = false;
    let secs = app.st.rec.since.take().map(|s| s.elapsed().as_secs()).unwrap_or(0);
    let path = std::mem::take(&mut app.st.rec.path);
    app.mark(topic::REC);
    crate::shell::restart_clock(app);
    if std::path::Path::new(&path).is_file() {
        let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        crate::actions::system_notice(app, "Recording saved", &format!("{name} · {}", crate::ui::fmt_time(secs as f64)));
    } else {
        crate::actions::system_notice(app, "Recording", "Nothing was recorded.");
    }
}
