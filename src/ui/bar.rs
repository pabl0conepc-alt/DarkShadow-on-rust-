//! The DarkShadow bar and its dynamic island.
//!
//! Styles: `island` (one floating pill in the middle, the default), `dynamic` (a tonal pill on each side and the
//! island in the middle), `flat` (one full-width band). The island is the clock; for a moment it becomes the
//! volume, the microphone, the brightness, the workspace or the charger, and the player / today / notifications
//! grow out of it as a card (card.rs).

use crate::color::{Rgba, rgba};
use crate::gfx::{Align, Canvas, Shape};
use crate::state::{NetKind, WmKind};
use crate::text::Face;
use crate::ui::{Act, CardMode, Ctx, Hits, IslandMode, Out, Ui};

const PAD: f32 = 5.0;
const ICON_W: f32 = 34.0;

fn module_width(cv: &mut Canvas, ctx: &Ctx, m: &str) -> f32 {
    let st = ctx.st;
    match m {
        "power" | "launcher" | "network" | "volume" | "notifications" | "settings" | "wallpaper" => ICON_W,
        "bluetooth" => {
            if st.bt.present {
                ICON_W
            } else {
                0.0
            }
        }
        "battery" => {
            if st.batt.present {
                ICON_W + cv.measure(&format!("{}%", st.batt.pct.round()), Face::Semi, 12.0) + 2.0
            } else {
                0.0
            }
        }
        "tray" => st.tray.len() as f32 * 30.0,
        "workspaces" => {
            if st.wm.kind == WmKind::None {
                0.0
            } else {
                let n = ws_count(ctx) as f32;
                n * 14.0 + 16.0 + 16.0
            }
        }
        "title" => {
            if st.wm.title.is_empty() {
                0.0
            } else {
                cv.measure(&st.wm.title, Face::Medium, 13.0).min(220.0) + 16.0
            }
        }
        "media" | "music" => {
            if st.media.active() {
                cv.measure(&st.media.title, Face::Medium, 13.0).min(180.0) + 40.0
            } else {
                0.0
            }
        }
        _ => 0.0,
    }
}

fn ws_count(ctx: &Ctx) -> usize {
    let st = ctx.st;
    let shown = ctx.cfg.bar.workspaces.clamp(1, 10) as usize;
    match st.wm.kind {
        WmKind::Drift => st.wm.workspaces.len().max(1),
        _ => {
            let max_id = st.wm.workspaces.iter().map(|w| w.id).max().unwrap_or(1).max(st.wm.active) as usize;
            shown.max(max_id.min(10))
        }
    }
}

/// The island's natural width for what it shows now.
fn island_target(cv: &mut Canvas, ctx: &Ctx, ui: &Ui) -> f32 {
    if !ctx.cfg.island.dynamic {
        return idle_width(cv, ctx);
    }
    match ui.island.mode {
        IslandMode::Idle if ctx.st.rec.on => 156.0,
        IslandMode::Idle => idle_width(cv, ctx),
        IslandMode::Volume | IslandMode::Mic | IslandMode::Bright => 236.0,
        IslandMode::Ws => 176.0,
        IslandMode::Charger => 206.0,
    }
}

fn clock_text(ctx: &Ctx) -> String {
    let t = crate::clock::now();
    let mut s = if ctx.cfg.bar.seconds { format!("{:02}:{:02}:{:02}", t.hour, t.min, t.sec) } else { format!("{:02}:{:02}", t.hour, t.min) };
    if ctx.cfg.bar.date {
        s = format!("{} {}  {s}", &crate::ui::weekday(t.wday)[..3], t.mday);
    }
    s
}

fn idle_width(cv: &mut Canvas, ctx: &Ctx) -> f32 {
    let mut w = 20.0 + cv.measure(&clock_text(ctx), Face::Semi, 14.0);
    if ctx.st.media.active() && ctx.cfg.island.media {
        w += 30.0;
    }
    if ctx.cfg.island.weather && ctx.st.weather.ok {
        w += 22.0 + cv.measure(&format!("{:.0}°", ctx.st.weather.temp), Face::Medium, 13.0);
    }
    if ctx.st.media.playing && ctx.cfg.island.wave {
        w += 26.0;
    }
    w
}


pub fn draw(cv: &mut Canvas, ctx: &Ctx, ui: &mut Ui, hits: &mut Hits) -> Out {
    let c = &ctx.cfg.bar;
    let p = ctx.th.pal;
    let now = ctx.now;
    let bh = c.height.clamp(28, 64) as f32;
    let w = cv.w();
    let h = cv.h();
    let top = c.position == "top";
    // auto-hide: the bar slides away; a thin strip at the screen edge brings it back
    let shown = if c.autohide { ui.bar_rev.value(now) } else { 1.0 };
    let off = (1.0 - shown) * (bh + 14.0);
    let y = if top { 6.0 - off } else { h - bh - 6.0 + off };
    let alpha = if ctx.cfg.theme.flat || ctx.st.toggles.gaming { 1.0 } else { c.opacity.clamp(0.2, 1.0) };
    let bg = p.surface.alpha(alpha);
    let style = c.style.as_str();

    // the island's width glides on a spring
    let target = island_target(cv, ctx, ui);
    ui.island.width.set(target, now);
    let iw = ui.island.width.step(now);
    let mut anim = !ui.island.width.settled() || ui.island.face.moving(now);

    let zones = [&c.left, &c.center, &c.right];
    let widths: Vec<Vec<(String, f32)>> = zones
        .iter()
        .map(|z| {
            z.iter()
                .map(|m| {
                    let mw = if m == "island" || m == "clock" { iw } else { module_width(cv, ctx, m) };
                    (m.clone(), mw)
                })
                .filter(|(_, mw)| *mw > 0.0)
                .collect()
        })
        .collect();
    let zone_w = |z: &Vec<(String, f32)>| -> f32 {
        if z.is_empty() { 0.0 } else { z.iter().map(|x| x.1).sum::<f32>() + (z.len() as f32 - 1.0) * 2.0 + PAD * 2.0 }
    };

    let mut pills: Vec<(f32, f32, f32, f32)> = Vec::new();
    let mut placed: Vec<(String, f32, f32)> = Vec::new(); // module, x, width
    match style {
        "dynamic" | "islands" => {
            let lw = zone_w(&widths[0]);
            let cw = zone_w(&widths[1]);
            let rw = zone_w(&widths[2]);
            let xs = [8.0, (w - cw) / 2.0, w - rw - 8.0];
            for (i, zw) in [lw, cw, rw].into_iter().enumerate() {
                if zw <= 0.0 {
                    continue;
                }
                pills.push((xs[i], y, zw, bh));
                let mut x = xs[i] + PAD;
                for (m, mw) in &widths[i] {
                    placed.push((m.clone(), x, *mw));
                    x += mw + 2.0;
                }
            }
        }
        "flat" => {
            pills.push((0.0, if top { 0.0 } else { h - bh - 8.0 }, w, bh + 8.0));
            let yy = if top { 4.0 } else { h - bh - 4.0 };
            let _ = yy;
            let lw = zone_w(&widths[0]);
            let cw = zone_w(&widths[1]);
            let rw = zone_w(&widths[2]);
            let xs = [8.0, (w - cw) / 2.0, w - rw - 8.0];
            for i in 0..3 {
                let mut x = xs[i] + PAD;
                for (m, mw) in &widths[i] {
                    placed.push((m.clone(), x, *mw));
                    x += mw + 2.0;
                }
            }
            let _ = (lw, rw);
        }
        _ => {
            // one island in the middle holding everything
            let all: Vec<(String, f32)> = widths.iter().flatten().cloned().collect();
            let gaps = (all.len() as f32 - 1.0).max(0.0) * 2.0;
            let tw = all.iter().map(|x| x.1).sum::<f32>() + gaps + PAD * 2.0 + 8.0;
            let x0 = ((w - tw) / 2.0).round();
            pills.push((x0, y, tw, bh));
            let mut x = x0 + PAD + 4.0;
            for (m, mw) in all {
                placed.push((m, x, mw));
                x += mw + 2.0;
            }
        }
    }

    // the island squeezes a little when something new lands in it
    let squeeze = ui.island.pulse.map(|t| crate::anim::progress(t, now, 260)).unwrap_or(1.0);
    if squeeze < 1.0 {
        anim = true;
    }

    // backgrounds (soft shadow first)
    for &(px, py, pw, ph) in &pills {
        let r = if style == "flat" { 0.0 } else { ph / 2.0 };
        if ctx.cfg.theme.shadows && style != "flat" && !ctx.st.toggles.gaming {
            cv.shadow(px, py + 2.0, pw, ph, r, 8.0, 0.55);
        }
        cv.rrect(px, py, pw, ph, r, bg);
        cv.rrect_stroke(px, py, pw, ph, r, 1.0, rgba(1.0, 1.0, 1.0, 0.06));
    }

    let cy = y + bh / 2.0;
    for (m, x, mw) in placed {
        match m.as_str() {
            "island" | "clock" => anim |= island(cv, ctx, ui, hits, x, y, mw, bh, squeeze),
            "workspaces" => workspaces(cv, ctx, hits, x, cy, mw),
            "tray" => {
                for (i, t) in st_tray(ctx).iter().enumerate() {
                    let ix = x + i as f32 * 30.0;
                    let hov = hits.add(ix, y + 3.0, 30.0, bh - 6.0, Act::TrayClick(t.key.clone()));
                    hits.with_right(Act::TrayMenu(t.key.clone()));
                    if hov {
                        cv.circle(ix + 15.0, cy, (bh - 8.0) / 2.0, p.card_hi);
                    }
                    match &t.icon {
                        Some(img) => cv.image_rrect(img, ix + 6.0, cy - 9.0, 18.0, 18.0, 3.0),
                        None => {
                            cv.circle(ix + 15.0, cy, 9.0, p.container);
                            cv.text(ix + 15.0, cy, &t.letter, Face::Bold, 10.0, p.ink_container, Align::Center);
                        }
                    }
                }
            }
            "title" => {
                hits.add(x, y, mw, bh, Act::None);
                cv.text_max(x + 8.0, cy, &ctx.st.wm.title, Face::Medium, 13.0, p.dim, Align::Left, mw - 16.0);
            }
            "media" | "music" => {
                let hov = hits.add(x, y + 4.0, mw, bh - 8.0, Act::PlayPause);
                hits.with_right(Act::OpenCard(CardMode::Player));
                if hov {
                    cv.rrect(x, y + 4.0, mw, bh - 8.0, (bh - 8.0) / 2.0, p.card_hi);
                }
                cv.icon(if ctx.st.media.playing { "pause" } else { "play_arrow" }, x + 16.0, cy, 18.0, p.fg);
                cv.text_max(x + 30.0, cy, &ctx.st.media.title, Face::Medium, 13.0, p.fg, Align::Left, mw - 38.0);
            }
            "battery" => {
                hits.add(x, y + 4.0, mw, bh - 8.0, Act::ToggleControl);
                let b = &ctx.st.batt;
                let col = if b.pct < 15.0 && !b.charging { p.warn } else { p.fg };
                cv.icon(b.icon(), x + ICON_W / 2.0, cy, 19.0, col);
                cv.text(x + ICON_W - 2.0, cy, &format!("{}%", b.pct.round()), Face::Semi, 12.0, col, Align::Left);
            }
            other => {
                let (icon, act, right, scroll) = icon_module(ctx, other);
                let hov = hits.add(x, y + 3.0, mw, bh - 6.0, act);
                if right != Act::None {
                    hits.with_right(right);
                }
                if scroll != Act::None {
                    hits.with_scroll(scroll);
                }
                if hov {
                    cv.circle(x + mw / 2.0, cy, (bh - 8.0) / 2.0, p.card_hi);
                }
                let col = match other {
                    "power" => p.fg,
                    "notifications" if !ctx.st.notifs.list.is_empty() && !ctx.st.toggles.dnd => p.primary,
                    _ => p.fg,
                };
                if other == "launcher" {
                    logo(cv, x + mw / 2.0, cy, ctx);
                } else {
                    cv.icon(icon, x + mw / 2.0, cy, 19.0, col);
                }
                if other == "notifications" && !ctx.st.notifs.list.is_empty() && !ctx.st.toggles.dnd {
                    cv.circle(x + mw / 2.0 + 7.0, cy - 7.0, 3.5, p.primary);
                }
            }
        }
    }

    if c.autohide {
        anim |= ui.bar_rev.animating(now);
        if shown < 0.05 && !ui.bar_rev.open {
            let strip = (0.0, if top { 0.0 } else { h - 3.0 }, w, 3.0);
            return Out { anim, input: Some(vec![strip]), done: false, damage: None };
        }
    }
    Out { anim, input: Some(pills.iter().map(|&(x, y, w, h)| (x, y - 2.0, w, h + 4.0)).collect()), done: false, damage: None }
}

fn icon_module(ctx: &Ctx, m: &str) -> (&'static str, Act, Act, Act) {
    let st = ctx.st;
    match m {
        "power" => ("power_settings_new", Act::TogglePower, Act::None, Act::None),
        "network" => (
            if st.net.ok || st.net.kind != NetKind::None { st.net.icon() } else { "wifi_off" },
            Act::ToggleControl,
            Act::Wifi,
            Act::None,
        ),
        "bluetooth" => (
            if !st.bt.powered {
                "bluetooth_disabled"
            } else if !st.bt.connected.is_empty() {
                "bluetooth_connected"
            } else {
                "bluetooth"
            },
            Act::ToggleControl,
            Act::Bluetooth,
            Act::None,
        ),
        "volume" => (
            if st.audio.muted || st.audio.vol <= 0.001 {
                "volume_off"
            } else if st.audio.vol < 0.4 {
                "volume_down"
            } else {
                "volume_up"
            },
            Act::ToggleControl,
            Act::Mute,
            Act::Volume,
        ),
        "notifications" => (
            if st.toggles.dnd {
                "notifications_off"
            } else if st.notifs.list.is_empty() {
                "notifications"
            } else {
                "notifications_active"
            },
            Act::ToggleControl,
            Act::Dnd,
            Act::None,
        ),
        "settings" => ("settings", Act::Settings, Act::None, Act::None),
        "wallpaper" => ("wallpaper", Act::OpenPicker, Act::WallShuffle, Act::None),
        _ => ("apps", Act::OpenLauncher, Act::None, Act::None),
    }
}

/// The launcher button: DarkShadow's black hole, drawn (a ring with a bright edge around a dark core).
fn logo(cv: &mut Canvas, cx: f32, cy: f32, ctx: &Ctx) {
    let p = ctx.th.pal;
    cv.ring(cx, cy, 7.2, 2.6, p.primary);
    cv.arc(cx, cy, 9.6, -2.6, 2.2, 1.6, p.primary.alpha(0.55));
    cv.circle(cx, cy, 3.4, p.fg);
}

#[allow(clippy::too_many_arguments)]
fn workspaces(cv: &mut Canvas, ctx: &Ctx, hits: &mut Hits, x: f32, cy: f32, mw: f32) {
    let p = ctx.th.pal;
    let st = ctx.st;
    let n = ws_count(ctx);
    hits.add(x, cy - 14.0, mw, 28.0, Act::None);
    hits.with_scroll(Act::WsRel(1));
    let mut xx = x + 8.0;
    for i in 0..n {
        let id = match st.wm.kind {
            WmKind::Drift => st.wm.workspaces.get(i).map(|w| w.id).unwrap_or(i as i32 + 1),
            _ => i as i32 + 1,
        };
        let occupied = st.wm.workspaces.iter().any(|w| w.id == id && w.windows > 0);
        let active = st.wm.active == id;
        let slot = if active { 30.0 } else { 14.0 };
        let hov = hits.add(xx, cy - 12.0, slot, 24.0, Act::Ws(id));
        hits.with_scroll(Act::WsRel(1));
        if active {
            cv.rrect(xx + 1.0, cy - 7.0, slot - 2.0, 14.0, 7.0, p.primary);
            if ctx.cfg.bar.ws_style == "numbers" {
                cv.text(xx + slot / 2.0, cy, &id.to_string(), Face::Bold, 10.0, p.ink_primary, Align::Center);
            }
        } else {
            let r = if occupied { 4.0 } else { 3.0 };
            let col = if occupied { p.fg } else { p.dim.alpha(0.55) };
            if hov {
                cv.circle(xx + slot / 2.0, cy, 7.0, p.card_hi);
            }
            cv.circle(xx + slot / 2.0, cy, r, col);
        }
        xx += slot;
    }
}

#[allow(clippy::too_many_arguments)]
fn island(cv: &mut Canvas, ctx: &Ctx, ui: &Ui, hits: &mut Hits, x: f32, y: f32, w: f32, h: f32, squeeze: f32) -> bool {
    let p = ctx.th.pal;
    let now = ctx.now;
    let st = ctx.st;
    let cy = y + h / 2.0;
    let mode = if ctx.cfg.island.dynamic { ui.island.mode } else { IslandMode::Idle };
    let k = ui.island.face.get(now);
    let mut anim = false;
    // the island as its own darker band only when it has grown into something
    let s = 1.0 - 0.08 * (1.0 - crate::anim::decel(squeeze));
    if mode != IslandMode::Idle {
        let ih = (h - 6.0) * s;
        cv.rrect(x, cy - ih / 2.0, w, ih, ih / 2.0, p.container.alpha(0.9));
    }
    match mode {
        IslandMode::Idle if st.rec.on => {
            // the screen is being recorded: a red dot that breathes, the time, stop
            let ih = h - 6.0;
            cv.rrect(x, cy - ih / 2.0, w, ih, ih / 2.0, p.container.alpha(0.9));
            let prev = cv.alpha;
            cv.alpha *= k;
            let red = Rgba::hex("#ff5f57").unwrap();
            // it breathes once a second, with the clock (no extra frames while you record)
            let secs = st.rec.since.map(|s| now.saturating_duration_since(s).as_secs()).unwrap_or(0);
            if secs % 2 == 0 {
                cv.circle(x + 20.0, cy, 9.0, red.alpha(0.25));
            }
            cv.circle(x + 20.0, cy, 5.0, red);
            let txt = format!("{:02}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60);
            cv.text(x + 36.0, cy, &txt, Face::Semi, 13.0, p.ink_container, Align::Left);
            let bx = x + w - 20.0;
            let hov = hits.add(bx - 16.0, cy - 14.0, 32.0, 28.0, Act::RecStop);
            cv.circle(bx, cy, if hov { 13.0 } else { 12.0 }, red);
            cv.rrect(bx - 4.5, cy - 4.5, 9.0, 9.0, 2.0, crate::color::WHITE);
            cv.alpha = prev;
        }
        IslandMode::Idle => {
            hits.add(x, y + 2.0, w, h - 4.0, Act::OpenCard(CardMode::Today));
            hits.with_right(Act::OpenCard(CardMode::System));
            hits.with_scroll(Act::Volume);
            let prev_alpha = cv.alpha;
            cv.alpha *= k;
            let mut xx = x + 10.0;
            if st.media.active() && ctx.cfg.island.media {
                let r = 11.0;
                match &st.media.art {
                    Some(img) => cv.image_shape(img, Shape::Cookie(9), xx + r, cy, r, 0.0),
                    None => {
                        cv.shape(Shape::Cookie(9), xx + r, cy, r, 0.0, p.container);
                        cv.icon("music_note", xx + r, cy, 13.0, p.ink_container);
                    }
                }
                xx += 2.0 * r + 8.0;
            }
            let t = clock_text(ctx);
            let tw = cv.text(xx, cy, &t, Face::Semi, 14.0, p.fg, Align::Left);
            xx += tw + 8.0;
            if ctx.cfg.island.weather && st.weather.ok {
                cv.icon(st.weather.icon, xx + 7.0, cy, 15.0, p.dim);
                xx += 16.0;
                let ww = cv.text(xx, cy, &format!("{:.0}°", st.weather.temp), Face::Medium, 13.0, p.dim, Align::Left);
                xx += ww + 8.0;
            }
            if st.media.playing && ctx.cfg.island.wave {
                // a tiny visualizer: four bars from cava (or a gentle idle motion)
                let n = 4;
                for i in 0..n {
                    let v = if st.viz_on && !st.viz.is_empty() {
                        let j = (i * st.viz.len() / n + st.viz.len() / 8).min(st.viz.len() - 1);
                        st.viz[j].clamp(0.08, 1.0)
                    } else {
                        0.35
                    };
                    let bh = 4.0 + 12.0 * v;
                    cv.rrect(xx + i as f32 * 5.0, cy - bh / 2.0, 3.0, bh, 1.5, p.primary);
                }
            }
            cv.alpha = prev_alpha;
        }
        IslandMode::Volume | IslandMode::Mic | IslandMode::Bright => {
            let (v, off, icon, act) = match mode {
                IslandMode::Mic => (
                    st.audio.mic,
                    st.audio.mic_muted,
                    if st.audio.mic_muted { "mic_off" } else { "mic" },
                    Act::Mic,
                ),
                IslandMode::Bright => (st.bright.pct, false, "brightness_medium", Act::Bright),
                _ => (
                    st.audio.vol,
                    st.audio.muted,
                    if st.audio.muted || st.audio.vol < 0.01 {
                        "volume_off"
                    } else if st.audio.vol < 0.4 {
                        "volume_down"
                    } else {
                        "volume_up"
                    },
                    Act::Volume,
                ),
            };
            let prev = cv.alpha;
            cv.alpha *= k;
            cv.icon(icon, x + 22.0, cy, 18.0, p.ink_container);
            let x0 = x + 42.0;
            let x1 = x + w - 46.0;
            hits.slider(x0 - 4.0, y, x1 - x0 + 8.0, h, act.clone());
            hits.with_scroll(act);
            let fx = x0 + (x1 - x0) * v.clamp(0.0, 1.0);
            cv.line(fx + 6.0, cy, x1, cy, 4.0, p.ink_container.alpha(0.25));
            let col = if off { p.dim } else { p.primary };
            if ctx.cfg.theme.wavy && !off {
                cv.wavy_line(x0, fx, cy, 3.0, 18.0, now_phase(ctx) * 2.0, 4.0, col);
                anim = true;
            } else {
                cv.line(x0, cy, fx, cy, 4.0, col);
            }
            cv.rrect(fx - 2.0, cy - 9.0, 4.0, 18.0, 2.0, col);
            cv.text(x + w - 16.0, cy, &format!("{}", (v * 100.0).round() as i32), Face::Semi, 13.0, p.ink_container, Align::Right);
            cv.alpha = prev;
        }
        IslandMode::Ws => {
            let prev = cv.alpha;
            cv.alpha *= k;
            hits.add(x, y, w, h, Act::None);
            hits.with_scroll(Act::WsRel(1));
            let name = st
                .wm
                .workspaces
                .iter()
                .find(|w| w.id == st.wm.active)
                .map(|w| w.name.clone())
                .filter(|n| !n.is_empty() && n.parse::<i32>().is_err())
                .unwrap_or_else(|| format!("Workspace {}", st.wm.active));
            cv.circle(x + 22.0, cy, 11.0, p.primary);
            cv.text(x + 22.0, cy, &st.wm.active.to_string(), Face::Bold, 12.0, p.ink_primary, Align::Center);
            cv.text_max(x + 42.0, cy, &name, Face::Semi, 13.0, p.ink_container, Align::Left, w - 54.0);
            cv.alpha = prev;
        }
        IslandMode::Charger => {
            let prev = cv.alpha;
            cv.alpha *= k;
            hits.add(x, y, w, h, Act::None);
            let b = &st.batt;
            let t = crate::anim::progress(ui.island.mode_at, now, 900);
            anim |= t < 1.0;
            let green = Rgba::hex("#7ddc8a").unwrap();
            let col = if b.plugged { green } else { p.ink_container };
            if b.plugged {
                // two rings spreading from the bolt
                for i in 0..2 {
                    let tt = (t - i as f32 * 0.25).clamp(0.0, 1.0);
                    if tt > 0.0 && tt < 1.0 {
                        cv.ring(x + 22.0, cy, 8.0 + 12.0 * tt, 2.0, green.alpha(1.0 - tt));
                    }
                }
            }
            cv.icon(if b.plugged { "bolt" } else { b.icon() }, x + 22.0, cy, 18.0, col);
            let x0 = x + 42.0;
            let x1 = x + w - 52.0;
            cv.rrect(x0, cy - 5.0, x1 - x0, 10.0, 5.0, p.ink_container.alpha(0.18));
            let fill = (x1 - x0) * (b.pct / 100.0).clamp(0.0, 1.0) * crate::anim::decel(t);
            cv.rrect(x0, cy - 5.0, fill.max(10.0), 10.0, 5.0, col);
            let shown = (b.pct * crate::anim::decel(t)).round();
            cv.text(x + w - 14.0, cy, &format!("{shown}%"), Face::Semi, 13.0, p.ink_container, Align::Right);
            cv.alpha = prev;
        }
    }
    anim
}

fn st_tray<'a>(ctx: &'a Ctx) -> &'a [crate::state::TrayIcon] {
    &ctx.st.tray
}

fn now_phase(ctx: &Ctx) -> f32 {
    -crate::ui::secs(ctx.now) * 2.4
}
