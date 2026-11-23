//! The audio visualizer: `cava` in raw ASCII mode, running only while music plays and something shows it.

use crate::app::App;
use crate::proc::{self, Stream};
use crate::state::topic;

#[derive(Default)]
pub struct Viz {
    stream: Option<Stream>,
    bars: u32,
}

fn wanted(app: &App) -> bool {
    let c = &app.cfg;
    if c.performance.gaming || app.st.toggles.gaming || !app.st.media.playing {
        return false;
    }
    let desk = c.widgets.enabled
        && c.widgets.visualizer.enabled
        && !(app.st.wm.covered && c.widgets.pause_covered);
    let island = c.bar.enabled && c.island.wave;
    desk || island
}

pub fn update_wanted(app: &mut App) {
    let want = wanted(app) && proc::which("cava");
    let bars = app.cfg.widgets.visualizer.bars.clamp(8, 200);
    if want && (app.svc.viz.stream.is_none() || app.svc.viz.bars != bars) {
        if let Some(s) = app.svc.viz.stream.take() {
            s.stop(app);
        }
        let conf = crate::config::runtime_dir().join("dshell-cava.conf");
        let text = format!(
            "[general]\nbars = {bars}\nframerate = {}\nsleep_timer = 2\n[input]\nmethod = pipewire\n[output]\nmethod = raw\nraw_target = /dev/stdout\ndata_format = ascii\nascii_max_range = 1000\nbar_delimiter = 59\nframe_delimiter = 10\n[smoothing]\nnoise_reduction = 70\n",
            app.cfg.widgets.visualizer.fps.clamp(10, 60)
        );
        let _ = std::fs::write(&conf, text);
        let c = conf.to_string_lossy().into_owned();
        app.svc.viz.bars = bars;
        app.svc.viz.stream = proc::stream(
            app,
            &["cava", "-p", &c],
            |app, line| {
                let v = &mut app.st.viz;
                v.clear();
                v.extend(
                    line.split(';')
                        .filter(|x| !x.is_empty())
                        .map(|x| x.parse::<f32>().unwrap_or(0.0) / 1000.0),
                );
                app.mark(topic::VIZ);
            },
            |app| {
                app.svc.viz.stream = None;
                app.st.viz_on = false;
            },
        );
        app.st.viz_on = app.svc.viz.stream.is_some();
        app.mark(topic::VIZ);
    } else if !want && let Some(s) = app.svc.viz.stream.take() {
        s.stop(app);
        app.st.viz_on = false;
        for v in app.st.viz.iter_mut() {
            *v = 0.0;
        }
        app.mark(topic::VIZ);
    }
}
