//! Desktop widgets (each its own small surface under the windows) and the visualizer strip.
//! Their colors turn light or dark with the wallpaper behind each one; their motion pauses while windows
//! cover the desktop.

use crate::color::{Rgba, rgba};
use crate::gfx::{Align, Canvas, Shape};
use crate::shell::SHADOW;
use crate::text::Face;
use crate::theme::WPal;
use crate::ui::{Act, Ctx, Hits, Out, Ui, WidgetId, fmt_time, month, weekday};

fn moving(ctx: &Ctx) -> bool {
    !(ctx.st.wm.covered && ctx.cfg.widgets.pause_covered) && !ctx.st.toggles.gaming && !ctx.cfg.performance.gaming
}

fn card(cv: &mut Canvas, ctx: &Ctx, wp: &WPal, x: f32, y: f32, w: f32, h: f32) {
    let r = wp.radius;
    if ctx.cfg.theme.shadows && wp.card.a > 0.05 && !ctx.st.toggles.gaming {
        cv.shadow(x, y + 3.0, w, h, r, 14.0, 0.5);
    }
    cv.rrect(x, y, w, h, r, wp.card);
    if wp.edge.a > 0.0 {
        cv.rrect_stroke(x, y, w, h, r, 1.0, wp.edge);
    }
    if wp.shine {
        cv.rrect_vgrad(x, y, w, h, r, rgba(1.0, 1.0, 1.0, 0.045), rgba(1.0, 1.0, 1.0, 0.0), 0.45);
    }
}

/// Where the widget is on the screen (for the wallpaper's brightness behind it).
fn screen_rect(ctx: &Ctx, id: WidgetId, w: f32, h: f32) -> (f32, f32) {
    let c = id.cfg(ctx.cfg);
    let (x, y) = (c.x as f32, c.y as f32);
    match c.anchor.as_str() {
        "tr" => (ctx.sw - x - w, y),
        "bl" => (x, ctx.sh - y - h),
        "br" => (ctx.sw - x - w, ctx.sh - y - h),
        _ => (x, y),
    }
}

pub fn draw(cv: &mut Canvas, ctx: &Ctx, _ui: &mut Ui, hits: &mut Hits, id: WidgetId) -> Out {
    let m = SHADOW as f32;
    let (w, h) = (cv.w() - 2.0 * m, cv.h() - 2.0 * m);
    let (sx, sy) = screen_rect(ctx, id, w, h);
    let lum = ctx.st.wall.lum_at(sx, sy, w, h, ctx.sw, ctx.sh);
    let light = ctx.cfg.theme.adapt && lum > 0.56;
    let wp = *ctx.th.w(light);
    let edit = ctx.edit;
    let mut hits_local = Hits::new(hits.hover, hits.pressed);
    let hh = if edit { &mut hits_local } else { &mut *hits };
    let mut damage = None;
    let anim = match id {
        WidgetId::Clock => clock(cv, ctx, &wp, m, m, w, h),
        WidgetId::User => user(cv, ctx, &wp, m, m, w, h),
        WidgetId::Trio => trio(cv, ctx, &wp, m, m, w, h),
        WidgetId::Focus => {
            let (cx, cy, r) = (m + w / 2.0, m + 126.0, 76.0);
            damage = Some(vec![(cx - r, cy - r, r * 2.0, r * 2.0)]);
            focus(cv, ctx, _ui, &wp, hh, m, m, w, h)
        }
        WidgetId::Player => {
            // only the cover, its ring and the line move
            let (cx, cy, r) = (m + w / 2.0, m + 118.0, 76.0 + 18.0);
            damage = Some(vec![(cx - r, cy - r, r * 2.0, r * 2.0), (m + 8.0, m + 252.0, w - 16.0, 28.0)]);
            player(cv, ctx, &wp, hh, m, m, w, h)
        }
        WidgetId::Weather => weather(cv, ctx, &wp, m, m, w, h),
        WidgetId::Calendar => calendar(cv, ctx, &wp, m, m, w, h),
    };
    if edit {
        // a frame and the widget's name; dragging moves it
        cv.rrect_stroke(m - 6.0, m - 6.0, w + 12.0, h + 12.0, 22.0, 1.5, wp.fg.alpha(0.55));
        let name = id.name();
        let lw = cv.measure(name, Face::Medium, 11.0) + 16.0;
        cv.rrect(m, m - 2.0, lw, 20.0, 10.0, rgba(0.0, 0.0, 0.0, 0.55));
        cv.text(m + 8.0, m + 8.0, name, Face::Medium, 11.0, crate::color::WHITE, Align::Left);
    }
    Out { anim: anim && moving(ctx), input: None, done: false, damage: if edit { None } else { damage } }
}

// ------------------------------------------------------------------ clock: two big lines

fn clock(cv: &mut Canvas, ctx: &Ctx, wp: &WPal, x: f32, y: f32, w: f32, h: f32) -> bool {
    let tm = crate::clock::now();
    let style = ctx.cfg.widgets.clock.style.as_str();
    match style {
        "stacked" => {
            card(cv, ctx, wp, x, y, w, h);
            cv.text(x + w / 2.0, y + h * 0.36, &format!("{:02}", tm.hour), Face::Numbers, 86.0, wp.fg, Align::Center);
            cv.text(x + w / 2.0, y + h * 0.7, &format!("{:02}", tm.min), Face::Numbers, 86.0, wp.primary, Align::Center);
        }
        _ => {
            // expressive: tall narrow digits straight on the wallpaper
            let size = h * 0.52;
            cv.text(x + w / 2.0, y + h * 0.25, &format!("{:02}", tm.hour), Face::Display, size, wp.fg, Align::Center);
            cv.text(x + w / 2.0, y + h * 0.74, &format!("{:02}", tm.min), Face::Display, size, wp.fg2.alpha(0.86), Align::Center);
        }
    }
    false
}

// ------------------------------------------------------------------ user: hi, name!

fn user(cv: &mut Canvas, ctx: &Ctx, wp: &WPal, x: f32, y: f32, w: f32, h: f32) -> bool {
    card(cv, ctx, wp, x, y, w, h);
    let u = &ctx.st.user;
    let (ax, ay, r) = (x + 18.0 + 38.0, y + h / 2.0, 38.0);
    match &u.avatar {
        Some(img) => cv.image_rrect(img, ax - r, ay - r, r * 2.0, r * 2.0, 20.0),
        None => {
            cv.shape(Shape::parse(&ctx.cfg.theme.shape), ax, ay, r, 0.0, wp.primary);
            let l: String = u.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
            cv.text(ax, ay, &l, Face::Bold, 28.0, wp.ink_on, Align::Center);
        }
    }
    let tm = crate::clock::now();
    let tx = ax + r + 18.0;
    let first = u.real.split_whitespace().next().map(str::to_string).unwrap_or_else(|| u.name.clone());
    cv.text_max(tx, ay - 20.0, &format!("Hi, {}!", first.to_lowercase()), Face::Bold, 17.0, wp.fg, Align::Left, w - (tx - x) - 12.0);
    cv.text(tx, ay + 3.0, crate::clock::greeting(tm.hour), Face::Regular, 14.0, wp.fg2, Align::Left);
    cv.text_max(
        tx,
        ay + 24.0,
        &format!("Today · {} {} {}", weekday(tm.wday), tm.mday, &month(tm.mon)[..3]),
        Face::Regular,
        11.0,
        wp.fg3,
        Align::Left,
        w - (tx - x) - 12.0,
    );
    false
}

// ------------------------------------------------------------------ CPU · GPU · RAM in soft shapes

fn trio(cv: &mut Canvas, ctx: &Ctx, wp: &WPal, x: f32, y: f32, w: f32, h: f32) -> bool {
    let s = &ctx.st.sys;
    let mut items: Vec<(&str, f32)> = vec![("CPU", s.cpu)];
    match (s.gpu, s.temp) {
        (Some(g), _) => items.push(("GPU", g)),
        (None, Some(t)) => items.push(("HEAT", (t / 100.0).clamp(0.0, 1.0))),
        _ => items.push(("DISK", s.disk)),
    }
    items.push(("RAM", s.ram));
    // the busiest one stands out in the warm color
    let hi = items.iter().enumerate().max_by(|a, b| a.1.1.partial_cmp(&b.1.1).unwrap()).map(|x| x.0).unwrap_or(1);
    let cell = w / 3.0;
    let r = (cell / 2.0 - 6.0).min(h / 2.0 - 4.0);
    let rot = 0.0;
    for (i, (label, v)) in items.iter().enumerate() {
        let cx = x + cell * (i as f32 + 0.5);
        let cy = y + h / 2.0;
        let strong = i == hi && *v > 0.25;
        if strong {
            cv.shape(Shape::Circle, cx, cy, r, 0.0, wp.tertiary);
        } else {
            cv.shape(Shape::Cookie(9), cx, cy, r, rot * if i == 0 { 1.0 } else { -1.0 }, wp.shape_fill);
        }
        let (fg, dim) = if strong { (wp.ink_tertiary, wp.ink_tertiary.alpha(0.75)) } else { (wp.fg, wp.fg3) };
        let txt = if *label == "HEAT" { format!("{:.0}°", s.temp.unwrap_or(0.0)) } else { format!("{}%", (v * 100.0).round()) };
        cv.text(cx, cy - 5.0, &txt, Face::Numbers, 22.0, fg, Align::Center);
        cv.text(cx, cy + 17.0, label, Face::Medium, 11.0, dim, Align::Center);
    }
    false
}

// ------------------------------------------------------------------ focus timer

#[allow(clippy::too_many_arguments)]
fn focus(cv: &mut Canvas, ctx: &Ctx, ui: &Ui, wp: &WPal, hits: &mut Hits, x: f32, y: f32, w: f32, h: f32) -> bool {
    let f = &ui.focus;
    let now = ctx.now;
    let (cx, cy, r) = (x + w / 2.0, y + 126.0, 66.0);
    let key = crate::ui::hkey(&(
        wp.light,
        f.running,
        f.on_break,
        f.minutes,
        f.break_minutes,
        f.done_sessions,
        hits.hover,
        hits.pressed,
        ctx.edit,
        &ctx.cfg.theme.widget_style,
        (wp.primary.g * 255.0) as u8,
    ));
    cv.cached(0x464f4355, key, hits, |cv, hits| {
        card(cv, ctx, wp, x, y, w, h);
        // Focus | Break
        let segw = 76.0;
        let sx = x + w / 2.0 - segw;
        for (i, (label, brk)) in [("Focus", false), ("Break", true)].iter().enumerate() {
            let bx = sx + i as f32 * segw;
            let on = f.on_break == *brk;
            let hov = hits.add(bx, y + 16.0, segw, 28.0, Act::FocusMode(*brk));
            if on {
                cv.rrect(bx + 2.0, y + 16.0, segw - 4.0, 28.0, 14.0, wp.fg);
            } else {
                cv.rrect(bx + 2.0, y + 16.0, segw - 4.0, 28.0, 14.0, if hov { wp.chip_hover } else { wp.chip });
            }
            cv.text(bx + segw / 2.0, y + 30.0, label, Face::Semi, 12.0, if on { wp.ink_on } else { wp.fg3 }, Align::Center);
        }
        let sub = if f.on_break { "breathe" } else if f.running { "stay with it" } else { "ready?" };
        cv.text(cx, cy + 24.0, sub, Face::Regular, 11.0, wp.fg3, Align::Center);
        // buttons
        let by = y + 234.0;
        crate::ui::icon_button(cv, hits, cx - 58.0, by, 17.0, "refresh", wp.fg2, wp.chip, wp.chip_hover, Act::FocusReset);
        let hov = hits.add(cx - 24.0, by - 24.0, 48.0, 48.0, Act::FocusToggle);
        cv.circle(cx, by, if hov { 25.0 } else { 24.0 }, wp.fg);
        cv.icon(if f.running { "pause" } else { "play_arrow" }, cx, by, 26.0, wp.ink_on);
        crate::ui::icon_button(cv, hits, cx + 58.0, by, 17.0, "skip_next", wp.fg2, wp.chip, wp.chip_hover, Act::FocusSkip);
        // sessions and length
        let dy = y + h - 22.0;
        for i in 0..4 {
            let on = (f.done_sessions % 4) > i;
            cv.circle(cx - 66.0 + i as f32 * 9.0, dy, 2.6, if on { wp.fg } else { wp.fg4 });
        }
        let minutes = if f.on_break { f.break_minutes } else { f.minutes };
        hits.add(cx - 22.0, dy - 10.0, 16.0, 20.0, Act::FocusMinutes(-1));
        cv.text(cx - 14.0, dy, "−", Face::Semi, 13.0, wp.fg2, Align::Center);
        cv.text(cx + 12.0, dy, &format!("{minutes} min"), Face::Medium, 11.0, wp.fg3, Align::Center);
        hits.add(cx + 32.0, dy - 10.0, 16.0, 20.0, Act::FocusMinutes(1));
        cv.text(cx + 40.0, dy, "+", Face::Semi, 13.0, wp.fg2, Align::Center);
    });
    // the wavy sun that turns while the clock runs, and the time
    let left = f.remaining(now);
    let total = f.total().max(1.0);
    let rot = if f.running && moving(ctx) { crate::ui::secs(now) * 0.35 } else { 0.0 };
    cv.wavy_arc(cx, cy, r - 4.0, 4.2, 14.0, rot * 6.0, 0.0, std::f32::consts::TAU, 5.0, wp.fg.alpha(if f.running { 1.0 } else { 0.92 }));
    if f.running || left < total - 0.5 {
        let done = 1.0 - (left / total) as f32;
        cv.arc(cx, cy, r - 14.0, -std::f32::consts::FRAC_PI_2, std::f32::consts::TAU * done, 3.0, wp.primary.alpha(0.8));
    }
    cv.text(cx, cy - 4.0, &fmt_time(left.ceil()), Face::Numbers, 34.0, wp.fg, Align::Center);
    f.running
}

// ------------------------------------------------------------------ the player

#[allow(clippy::too_many_arguments)]
fn player(cv: &mut Canvas, ctx: &Ctx, wp: &WPal, hits: &mut Hits, x: f32, y: f32, w: f32, h: f32) -> bool {
    let m = &ctx.st.media;
    let now = ctx.now;
    let mv = moving(ctx) && m.playing;
    let (cx, cy, r) = (x + w / 2.0, y + 118.0, 76.0);
    let ly = y + 266.0;
    let (x0, x1) = (x + 20.0, x + w - 20.0);
    // the still part: card, title, buttons (cached while the cover turns and the waves move)
    let key = crate::ui::hkey(&(
        wp.light,
        &m.title,
        &m.artist,
        m.playing,
        m.active(),
        hits.hover,
        hits.pressed,
        ctx.edit,
        &ctx.cfg.theme.widget_style,
        (wp.fg.r * 255.0) as u8,
        (wp.primary.g * 255.0) as u8,
    ));
    cv.cached(0x504c4159, key, hits, |cv, hits| {
        card(cv, ctx, wp, x, y, w, h);
        cv.ring(cx, cy, r + 12.0, 2.0, wp.track);
        let line = if !m.active() {
            "Nothing playing".to_string()
        } else if m.artist.is_empty() {
            m.title.clone()
        } else {
            format!("{} – {}", m.title, m.artist)
        };
        cv.text_max(x + 20.0, y + 236.0, &line, Face::Semi, 14.0, wp.fg, Align::Left, w - 40.0);
        hits.slider(x0 - 6.0, ly - 12.0, x1 - x0 + 12.0, 24.0, Act::Seek);
        cv.circle(x1, ly, 2.0, wp.fg2);
        let by = y + h - 46.0;
        let mid = x + w / 2.0;
        crate::ui::icon_button(cv, hits, mid - 104.0, by, 17.0, "skip_previous", wp.fg2, wp.chip, wp.chip_hover, Act::Prev);
        crate::ui::icon_button(cv, hits, mid - 58.0, by, 17.0, "fast_rewind", wp.fg2, wp.chip, wp.chip_hover, Act::SeekRel(-10.0));
        let hov = hits.add(mid - 32.0, by - 20.0, 64.0, 40.0, Act::PlayPause);
        cv.rrect(mid - 32.0, by - 20.0, 64.0, 40.0, 20.0, if hov { wp.fg.alpha(0.9) } else { wp.fg });
        cv.icon(if m.playing { "pause" } else { "play_arrow" }, mid, by, 24.0, wp.ink_on);
        crate::ui::icon_button(cv, hits, mid + 58.0, by, 17.0, "fast_forward", wp.fg2, wp.chip, wp.chip_hover, Act::SeekRel(10.0));
        crate::ui::icon_button(cv, hits, mid + 104.0, by, 17.0, "skip_next", wp.fg2, wp.chip, wp.chip_hover, Act::Next);
    });
    // what moves: the cover, the ring and the line
    let rot = if mv { crate::ui::secs(now) * 0.2 } else { 0.0 };
    let frac = ctx.seek.unwrap_or(m.fraction(now));
    if m.active() {
        cv.wavy_arc(
            cx,
            cy,
            r + 12.0,
            if ctx.cfg.theme.wavy { 2.6 } else { 0.0 },
            14.0,
            if mv { -crate::ui::secs(now) * 1.6 } else { 0.0 },
            -std::f32::consts::FRAC_PI_2,
            std::f32::consts::TAU * frac,
            3.0,
            wp.fg,
        );
    }
    match &m.art {
        Some(img) => cv.image_shape(img, Shape::Cookie(9), cx, cy, r, rot),
        None => {
            cv.shape(Shape::Cookie(9), cx, cy, r, rot, wp.shape_fill);
            cv.icon("music_note", cx, cy, 46.0, wp.fg3);
        }
    }
    let fx = x0 + (x1 - x0) * frac;
    cv.line((fx + 8.0).min(x1), ly, x1 - 6.0, ly, 3.0, wp.track);
    if ctx.cfg.theme.wavy && mv {
        cv.wavy_line(x0, fx, ly, 3.0, 22.0, -crate::ui::secs(now) * 3.0, 3.0, wp.fg);
    } else {
        cv.line(x0, ly, fx, ly, 3.0, wp.fg);
    }
    cv.rrect(fx - 2.0, ly - 10.0, 4.0, 20.0, 2.0, wp.fg);
    m.playing
}

// ------------------------------------------------------------------ weather and calendar

fn weather(cv: &mut Canvas, ctx: &Ctx, wp: &WPal, x: f32, y: f32, w: f32, h: f32) -> bool {
    card(cv, ctx, wp, x, y, w, h);
    let wst = &ctx.st.weather;
    if !wst.ok {
        cv.icon("cloud", x + w / 2.0, y + h / 2.0 - 10.0, 34.0, wp.fg3);
        cv.text(x + w / 2.0, y + h / 2.0 + 26.0, "Weather loading…", Face::Regular, 12.0, wp.fg3, Align::Center);
        return false;
    }
    cv.shape(Shape::Cookie(7), x + 58.0, y + h / 2.0, 38.0, 0.0, wp.shape_fill);
    cv.icon(wst.icon, x + 58.0, y + h / 2.0, 40.0, wp.primary);
    cv.text(x + 112.0, y + 50.0, &format!("{:.0}°", wst.temp), Face::Numbers, 38.0, wp.fg, Align::Left);
    cv.text_max(x + 112.0, y + 86.0, &wst.desc, Face::Medium, 13.0, wp.fg2, Align::Left, w - 128.0);
    cv.text_max(x + 112.0, y + 108.0, &format!("{} · ↑{:.0}° ↓{:.0}°", wst.city, wst.max, wst.min), Face::Regular, 11.0, wp.fg3, Align::Left, w - 128.0);
    false
}

fn calendar(cv: &mut Canvas, ctx: &Ctx, wp: &WPal, x: f32, y: f32, w: f32, h: f32) -> bool {
    card(cv, ctx, wp, x, y, w, h);
    let tm = crate::clock::now();
    crate::ui::card::calendar(cv, ctx, x + 14.0, y + 14.0, w - 28.0, &tm, wp.fg, wp.fg3, wp.primary, wp.ink_on);
    let _ = h;
    false
}

// ------------------------------------------------------------------ the visualizer strip

pub fn draw_viz(cv: &mut Canvas, ctx: &Ctx, _ui: &mut Ui) -> Out {
    let st = ctx.st;
    if !st.viz_on || st.viz.is_empty() || !moving(ctx) {
        return Out { anim: false, input: Some(vec![]), done: false, damage: None };
    }
    let vc = &ctx.cfg.widgets.visualizer;
    let (w, h) = (cv.w(), cv.h());
    let n = st.viz.len();
    let slot = w / n as f32;
    let bw = (slot * 0.62).max(2.0);
    let acc = ctx.th.accent_hi;
    let tert = ctx.th.pal.tertiary;
    let color = |i: usize| -> Rgba {
        let t = i as f32 / n.max(1) as f32;
        match vc.color.as_str() {
            "gradient" => acc.mix(tert, t),
            "rainbow" => Rgba::hsla(t, 0.7, 0.68, 1.0),
            "mono" => rgba(1.0, 1.0, 1.0, 1.0),
            _ => acc,
        }
    };
    match vc.style.as_str() {
        "wave" => {
            let mut pb = tiny_skia::PathBuilder::new();
            pb.move_to(0.0, h);
            for (i, v) in st.viz.iter().enumerate() {
                pb.line_to(slot * (i as f32 + 0.5), h - 4.0 - v.clamp(0.0, 1.0) * (h - 8.0));
            }
            pb.line_to(w, h);
            pb.close();
            if let Some(p) = pb.finish() {
                cv.fill_path(&p, acc.alpha(0.55));
            }
        }
        "mirror" => {
            for (i, v) in st.viz.iter().enumerate() {
                let bh = (v.clamp(0.0, 1.0) * (h - 6.0)).max(4.0);
                let x = slot * i as f32 + (slot - bw) / 2.0;
                cv.rrect(x, (h - bh) / 2.0, bw, bh, bw / 2.0, color(i).alpha(0.85));
            }
        }
        "dots" => {
            for (i, v) in st.viz.iter().enumerate() {
                let k = (v.clamp(0.0, 1.0) * 6.0).round() as i32;
                let x = slot * (i as f32 + 0.5);
                for d in 0..=k {
                    cv.circle(x, h - 6.0 - d as f32 * (bw + 3.0), bw / 2.0, color(i).alpha(0.9));
                }
            }
        }
        _ => {
            for (i, v) in st.viz.iter().enumerate() {
                let bh = (v.clamp(0.0, 1.0) * (h - 4.0)).max(bw.min(10.0));
                let x = slot * i as f32 + (slot - bw) / 2.0;
                cv.rrect(x, h - bh, bw, bh + bw, bw / 2.0, color(i).alpha(0.85));
            }
        }
    }
    // frames come from cava; no frame callbacks needed
    Out { anim: false, input: Some(vec![]), done: false, damage: None }
}
