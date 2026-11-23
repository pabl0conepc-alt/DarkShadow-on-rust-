//! Sound: PipeWire through `wpctl`; `pactl subscribe` tells us when something changed (no polling).

use std::time::Duration;

use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};

use crate::app::App;
use crate::proc::{self, Stream};
use crate::state::topic;

#[derive(Default)]
pub struct AudioSvc {
    sub: Option<Stream>,
    busy: bool,
    again: bool,
    armed: bool,
    set_busy: bool,
    pending: Option<(bool, f32)>,
    /// changes we made ourselves don't pop the island up a second time
    pub quiet_until: Option<std::time::Instant>,
}

pub fn start(app: &mut App) {
    if !proc::which("wpctl") {
        return;
    }
    app.st.audio.ok = true;
    refresh(app);
    if proc::which("pactl") {
        subscribe(app);
    }
}

fn subscribe(app: &mut App) {
    app.svc.audio.sub = proc::stream(
        app,
        &["pactl", "subscribe"],
        |app, line| {
            if line.contains("sink") || line.contains("source") || line.contains("server") {
                arm(app);
            }
        },
        |app| {
            app.svc.audio.sub = None;
            let _ = app.lh.insert_source(
                Timer::from_duration(Duration::from_secs(3)),
                |_, _, app: &mut App| {
                    subscribe(app);
                    TimeoutAction::Drop
                },
            );
        },
    );
}

fn arm(app: &mut App) {
    if app.svc.audio.armed {
        return;
    }
    app.svc.audio.armed = true;
    let _ = app.lh.insert_source(
        Timer::from_duration(Duration::from_millis(25)),
        |_, _, app: &mut App| {
            app.svc.audio.armed = false;
            refresh(app);
            TimeoutAction::Drop
        },
    );
}

fn parse(line: &str) -> Option<(f32, bool)> {
    let v = line.split_whitespace().nth(1)?.parse::<f32>().ok()?;
    Some((v, line.contains("MUTED")))
}

pub fn refresh(app: &mut App) {
    if app.svc.audio.busy {
        app.svc.audio.again = true;
        return;
    }
    app.svc.audio.busy = true;
    proc::capture(
        app,
        &[
            "sh",
            "-c",
            "wpctl get-volume @DEFAULT_AUDIO_SINK@; echo; wpctl get-volume @DEFAULT_AUDIO_SOURCE@",
        ],
        |app, out| {
            app.svc.audio.busy = false;
            let mut lines = out.lines().filter(|l| l.starts_with("Volume"));
            let old = (app.st.audio.vol, app.st.audio.muted, app.st.audio.mic_muted);
            if let Some((v, m)) = lines.next().and_then(parse) {
                if !app.svc.audio.set_busy {
                    app.st.audio.vol = v;
                }
                app.st.audio.muted = m;
            }
            if let Some((v, m)) = lines.next().and_then(parse) {
                app.st.audio.mic = v;
                app.st.audio.mic_muted = m;
            }
            let new = (app.st.audio.vol, app.st.audio.muted, app.st.audio.mic_muted);
            if old != new {
                app.mark(topic::AUDIO);
                let quiet = app
                    .svc
                    .audio
                    .quiet_until
                    .map(|t| t > std::time::Instant::now())
                    .unwrap_or(false);
                if !quiet && app.started.elapsed() > Duration::from_secs(2) {
                    let mode = if old.2 != new.2 {
                        crate::ui::IslandMode::Mic
                    } else {
                        crate::ui::IslandMode::Volume
                    };
                    crate::actions::island_flash(app, mode);
                }
            }
            if std::mem::take(&mut app.svc.audio.again) {
                refresh(app);
            }
        },
    );
}

/// Set the volume (or the microphone). While one change is on its way, only the newest waits.
pub fn set(app: &mut App, mic: bool, v: f32) {
    let v = v.clamp(0.0, 1.0);
    if mic {
        app.st.audio.mic = v;
    } else {
        app.st.audio.vol = v;
    }
    app.svc.audio.quiet_until = Some(std::time::Instant::now() + Duration::from_millis(600));
    app.mark(topic::AUDIO);
    if app.svc.audio.set_busy {
        app.svc.audio.pending = Some((mic, v));
        return;
    }
    send(app, mic, v);
}

fn send(app: &mut App, mic: bool, v: f32) {
    app.svc.audio.set_busy = true;
    let dev = if mic {
        "@DEFAULT_AUDIO_SOURCE@"
    } else {
        "@DEFAULT_AUDIO_SINK@"
    };
    let val = format!("{:.3}", v);
    proc::capture(
        app,
        &["wpctl", "set-volume", "-l", "1.0", dev, &val],
        |app, _| {
            app.svc.audio.set_busy = false;
            if let Some((m, v)) = app.svc.audio.pending.take() {
                send(app, m, v);
            }
        },
    );
}

pub fn step(app: &mut App, d: f32) {
    let v = (app.st.audio.vol + d).clamp(0.0, 1.0);
    set(app, false, v);
    if app.st.audio.muted && d > 0.0 {
        toggle_mute(app, false);
    }
}

pub fn toggle_mute(app: &mut App, mic: bool) {
    let dev = if mic {
        "@DEFAULT_AUDIO_SOURCE@"
    } else {
        "@DEFAULT_AUDIO_SINK@"
    };
    if mic {
        app.st.audio.mic_muted = !app.st.audio.mic_muted;
    } else {
        app.st.audio.muted = !app.st.audio.muted;
    }
    app.mark(topic::AUDIO);
    proc::capture(app, &["wpctl", "set-mute", dev, "toggle"], |app, _| {
        refresh(app)
    });
}
