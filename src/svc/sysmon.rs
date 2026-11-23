//! CPU, memory, GPU, heat and disk — read from /proc and /sys only while something on screen shows them.

use std::time::Duration;

use smithay_client_toolkit::reexports::calloop::{
    RegistrationToken,
    timer::{TimeoutAction, Timer},
};

use crate::app::App;
use crate::state::topic;

#[derive(Default)]
pub struct Sysmon {
    token: Option<RegistrationToken>,
    prev: Option<(u64, u64)>,
    gpu_path: Option<Option<String>>,
    temp_path: Option<Option<String>>,
}

pub fn start(app: &mut App) {
    update_wanted(app);
}

fn wanted(app: &App) -> bool {
    let w = &app.cfg.widgets;
    let desk = w.enabled && w.trio.enabled && !(app.st.wm.covered && w.pause_covered);
    let card = app.has(crate::ui::Kind::Card) && app.ui.card.mode == crate::ui::CardMode::System;
    let cc = app.has(crate::ui::Kind::Control) && app.ui.control.tab == 1;
    desk || card || cc
}

/// Start or stop the 2-second tick depending on whether anything shows the numbers.
pub fn update_wanted(app: &mut App) {
    let want = wanted(app);
    if want && app.svc.sysmon.token.is_none() {
        sample(app);
        let t = app.lh.insert_source(Timer::from_duration(Duration::from_secs(2)), |_, _, app: &mut App| {
            sample(app);
            TimeoutAction::ToDuration(Duration::from_secs(2))
        });
        app.svc.sysmon.token = t.ok();
    } else if !want && let Some(t) = app.svc.sysmon.token.take() {
        app.lh.remove(t);
        app.svc.sysmon.prev = None;
    }
}

fn read(p: &str) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

fn find_gpu() -> Option<String> {
    for i in 0..4 {
        let p = format!("/sys/class/drm/card{i}/device/gpu_busy_percent");
        if std::path::Path::new(&p).exists() {
            return Some(p);
        }
    }
    None
}

fn find_temp() -> Option<String> {
    let rd = std::fs::read_dir("/sys/class/hwmon").ok()?;
    let mut fallback = None;
    for e in rd.flatten() {
        let name = read(&format!("{}/name", e.path().display())).trim().to_string();
        let p = format!("{}/temp1_input", e.path().display());
        if !std::path::Path::new(&p).exists() {
            continue;
        }
        if matches!(name.as_str(), "k10temp" | "coretemp" | "zenpower" | "cpu_thermal" | "acpitz") {
            if name != "acpitz" {
                return Some(p);
            }
            fallback = Some(p);
        }
    }
    fallback
}

pub fn sample(app: &mut App) {
    let s = &mut app.svc.sysmon;
    // CPU from the first line of /proc/stat
    let stat = read("/proc/stat");
    if let Some(l) = stat.lines().next() {
        let v: Vec<u64> = l.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
        if v.len() >= 5 {
            let idle = v[3] + v.get(4).copied().unwrap_or(0);
            let total: u64 = v.iter().take(8).sum();
            if let Some((pi, pt)) = s.prev {
                let dt = total.saturating_sub(pt).max(1);
                let di = idle.saturating_sub(pi);
                app.st.sys.cpu = (1.0 - di as f32 / dt as f32).clamp(0.0, 1.0);
            }
            s.prev = Some((idle, total));
        }
    }
    let mem = read("/proc/meminfo");
    let field = |k: &str| {
        mem.lines()
            .find(|l| l.starts_with(k))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|x| x.parse::<f32>().ok())
            .unwrap_or(0.0)
    };
    let total = field("MemTotal:");
    let avail = field("MemAvailable:");
    if total > 0.0 {
        app.st.sys.ram = ((total - avail) / total).clamp(0.0, 1.0);
        app.st.sys.ram_used = (total - avail) / 1048576.0;
        app.st.sys.ram_total = total / 1048576.0;
    }
    if s.gpu_path.is_none() {
        s.gpu_path = Some(find_gpu());
    }
    if let Some(Some(p)) = &s.gpu_path {
        app.st.sys.gpu = read(p).trim().parse::<f32>().ok().map(|v| v / 100.0);
    }
    if s.temp_path.is_none() {
        s.temp_path = Some(find_temp());
    }
    if let Some(Some(p)) = &s.temp_path {
        app.st.sys.temp = read(p).trim().parse::<f32>().ok().map(|v| v / 1000.0);
    }
    // the root disk
    // SAFETY: statvfs fills the struct we pass.
    unsafe {
        let mut sv: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c"/".as_ptr(), &mut sv) == 0 {
            let total = sv.f_blocks as f64 * sv.f_frsize as f64;
            let free = sv.f_bavail as f64 * sv.f_frsize as f64;
            if total > 0.0 {
                app.st.sys.disk = ((total - free) / total) as f32;
                app.st.sys.disk_used = ((total - free) / 1e9) as f32;
                app.st.sys.disk_total = (total / 1e9) as f32;
            }
        }
    }
    app.mark(topic::SYS);
}
