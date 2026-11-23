//! The card that grows out of the island: Today (calendar, weather, the day as a wave), System (wavy rings),
//! the player, and notifications. It morphs from the island's size and place into the card (with a small
//! overshoot), then its content fades in.

use std::time::Instant;

use crate::color::{Rgba, rgba};
use crate::gfx::{Align, Canvas, Shape};
use crate::shell::SHADOW;
use crate::text::Face;
use crate::ui::{Act, CardMode, Ctx, Hits, Out, Ui, fmt_time, icon_button, month, weekday};

/// Which pieces to draw: the still ones go into a cached layer, the moving ones on top every frame.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Part {
    All,
    Still,
    Moving,
}

impl Part {
    fn still(self) -> bool {
        self != Part::Moving
    }
    fn moving(self) -> bool {
        self != Part::Still
    }
}

/// Everything the still part of the card shows (when it changes, the cached layer is redrawn).
fn content_key(ctx: &Ctx, ui: &Ui, hits: &Hits) -> u64 {
    let st = ctx.st;
    let m = &st.media;
    let tm = crate::clock::now();
    let n = ui
        .card
        .notif
        .or(st.notifs.popup)
        .and_then(|id| st.notifs.list.iter().find(|n| n.id == id));
    crate::ui::hkey(&(
        ui.card.mode as u8,
        ui.card.tab,
        hits.hover,
        hits.pressed,
        (tm.mday, tm.hour, tm.min),
        (st.weather.ok, st.weather.temp as i32, &st.weather.desc),
        (
            &m.title,
            &m.artist,
            m.playing,
            m.art.as_ref().map(|a| std::rc::Rc::as_ptr(a) as usize),
            m.position(ctx.now) as u64,
            m.length_us,
        ),
        n.map(|n| {
            (
                n.id,
                n.image.is_some(),
                n.actions.len(),
                n.at.elapsed().as_secs() / 60,
            )
        }),
        (
            (st.sys.cpu * 100.0) as u8,
            (st.sys.ram * 100.0) as u8,
            st.sys.gpu.map(|g| (g * 100.0) as u8),
            (st.sys.disk * 100.0) as u8,
        ),
        (
            (ctx.th.pal.primary.r * 255.0) as u8,
            (ctx.th.pal.primary.g * 255.0) as u8,
            (ctx.th.pal.surface.a * 255.0) as u8,
        ),
    ))
}

fn size(mode: CardMode) -> (f32, f32) {
    match mode {
        CardMode::Notif => (440.0, 104.0),
        CardMode::Player => (470.0, 188.0),
        _ => (520.0, 318.0),
    }
}

pub fn draw(cv: &mut Canvas, ctx: &Ctx, ui: &mut Ui, hits: &mut Hits) -> Out {
    let now = ctx.now;
    let p = ctx.th.pal;
    let c = &ui.card;
    let raw = c.rev.raw(now);
    let t = if c.rev.open {
        crate::anim::overshoot(raw)
    } else {
        1.0 - crate::anim::accel(1.0 - raw)
    };
    let anim = c.rev.animating(now);
    if c.rev.gone(now) {
        return Out {
            anim: false,
            input: Some(vec![]),
            done: true,
            damage: None,
        };
    }
    let top = ctx.cfg.bar.position == "top";
    let (cw, ch) = size(c.mode);
    let w = cv.w();
    let h = cv.h();
    let m = SHADOW as f32;
    // the end rect: just above (or below) the bar; the start rect: the island itself
    let fx = (w - cw) / 2.0;
    let fy = if top { m } else { h - m - ch };
    let (sw, sh) = (ui.island.width.x.max(120.0), 30.0);
    let sx = (w - sw) / 2.0;
    let sy = if top { -sh * 0.6 } else { h - sh * 0.4 };
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    let (x, y, cw2, ch2) = (
        lerp(sx, fx),
        lerp(sy, fy),
        lerp(sw, cw).max(10.0),
        lerp(sh, ch).max(10.0),
    );
    let r = lerp(sh / 2.0, 28.0);
    let fade = ((raw - 0.45) / 0.55).clamp(0.0, 1.0);
    let content_k = if c.rev.open {
        crate::anim::decel(fade)
    } else {
        raw * raw
    };

    let bg = if c.mode == CardMode::Player {
        p.surface.mix(
            Rgba::hsla(ctx.th.accent.hsl().0.max(0.0), 0.35, 0.16, p.surface.a),
            0.5,
        )
    } else {
        p.surface
    };
    let prev = cv.alpha;
    let mode = c.mode;
    let settled = !anim && content_k > 0.999 && t >= 0.999;
    let mut more_anim = false;
    let uir: &Ui = ui;
    if settled {
        // still: the card and everything that does not move, from the cache
        let key = content_key(ctx, uir, hits);
        cv.cached(0x43415244, key, hits, |cv, hits| {
            if ctx.cfg.theme.shadows {
                cv.shadow(x, y + 3.0, cw2, ch2, r, 14.0, 0.6);
            }
            cv.rrect(x, y, cw2, ch2, r, bg);
            cv.rrect_stroke(x, y, cw2, ch2, r, 1.0, p.line);
            content(cv, ctx, uir, hits, mode, fx, fy, cw, ch, Part::Still);
        });
        more_anim = content(cv, ctx, uir, hits, mode, fx, fy, cw, ch, Part::Moving);
    } else {
        cv.alpha *= t.clamp(0.0, 1.0).sqrt();
        if ctx.cfg.theme.shadows {
            cv.shadow(x, y + 3.0, cw2, ch2, r, 14.0, 0.6);
        }
        cv.rrect(x, y, cw2, ch2, r, bg);
        cv.rrect_stroke(x, y, cw2, ch2, r, 1.0, p.line);
        cv.alpha = prev * content_k;
        if content_k > 0.01 {
            more_anim = content(cv, ctx, uir, hits, mode, fx, fy, cw, ch, Part::All);
        }
    }
    cv.alpha = prev;
    Out {
        anim: anim || more_anim,
        input: Some(vec![(fx, fy, cw, ch)]),
        done: false,
        damage: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn content(
    cv: &mut Canvas,
    ctx: &Ctx,
    ui: &Ui,
    hits: &mut Hits,
    mode: CardMode,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    part: Part,
) -> bool {
    match mode {
        CardMode::Today | CardMode::System => info(cv, ctx, ui, hits, x, y, w, h, part),
        CardMode::Player => player(cv, ctx, hits, x, y, w, h, part),
        CardMode::Notif => notif(cv, ctx, ui, hits, x, y, w, h, part),
    }
}

#[allow(clippy::too_many_arguments)]
fn tabs(
    cv: &mut Canvas,
    ctx: &Ctx,
    hits: &mut Hits,
    x: f32,
    y: f32,
    labels: &[&str],
    sel: u8,
    act: fn(u8) -> Act,
) -> f32 {
    let p = ctx.th.pal;
    let mut xx = x;
    for (i, l) in labels.iter().enumerate() {
        let tw = cv.measure(l, Face::Semi, 13.0) + 30.0;
        let on = sel as usize == i;
        let hov = hits.add(xx, y, tw, 36.0, act(i as u8));
        if on {
            cv.rrect(xx, y, tw, 36.0, 18.0, p.primary);
        } else if hov {
            cv.rrect(xx, y, tw, 36.0, 18.0, rgba(1.0, 1.0, 1.0, 0.06));
        }
        cv.text(
            xx + tw / 2.0,
            y + 18.0,
            l,
            Face::Semi,
            13.0,
            if on { p.ink_primary } else { p.dim },
            Align::Center,
        );
        xx += tw + 4.0;
    }
    xx - x
}

#[allow(clippy::too_many_arguments)]
fn info(
    cv: &mut Canvas,
    ctx: &Ctx,
    ui: &Ui,
    hits: &mut Hits,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    part: Part,
) -> bool {
    let p = ctx.th.pal;
    let tab = ui.card.tab;
    let body_y = y + 62.0;
    if part.still() {
        tabs(
            cv,
            ctx,
            hits,
            x + 16.0,
            y + 14.0,
            &["Today", "System"],
            tab,
            Act::CardTab,
        );
        icon_button(
            cv,
            hits,
            x + w - 34.0,
            y + 32.0,
            18.0,
            "close",
            p.dim,
            rgba(0.0, 0.0, 0.0, 0.0),
            p.card_hi,
            Act::CloseAll,
        );
    }
    if tab == 1 {
        return system(cv, ctx, x, body_y, w, h - 62.0, part);
    }
    let tm = crate::clock::now();
    let lx = x + 24.0;
    let dy = y + h - 34.0;
    let frac = (tm.hour as f32 * 60.0 + tm.min as f32) / 1440.0;
    let (x0, x1) = (lx, x + w * 0.46);
    let fxp = x0 + (x1 - x0) * frac;
    if part.moving() {
        cv.wavy_line(
            x0,
            fxp,
            dy,
            3.0,
            16.0,
            -crate::ui::secs(ctx.now) * 1.5,
            4.0,
            p.primary,
        );
    }
    if !part.still() {
        return true;
    }
    // left: the day, the weather and how much of the day has gone
    cv.text(
        lx,
        body_y + 20.0,
        weekday(tm.wday),
        Face::Medium,
        15.0,
        p.dim,
        Align::Left,
    );
    cv.text(
        lx,
        body_y + 56.0,
        &format!("{} {}", tm.mday, month(tm.mon)),
        Face::Bold,
        30.0,
        p.fg,
        Align::Left,
    );
    let wy = body_y + 104.0;
    let wst = &ctx.st.weather;
    if wst.ok {
        cv.shape(
            Shape::Cookie(7),
            lx + 24.0,
            wy + 14.0,
            24.0,
            0.0,
            p.container,
        );
        cv.icon(wst.icon, lx + 24.0, wy + 14.0, 26.0, p.ink_container);
        cv.text(
            lx + 58.0,
            wy + 4.0,
            &format!("{:.0}°{}", wst.temp, wst.unit),
            Face::Bold,
            20.0,
            p.fg,
            Align::Left,
        );
        cv.text_max(
            lx + 58.0,
            wy + 26.0,
            &wst.desc,
            Face::Regular,
            12.0,
            p.dim,
            Align::Left,
            170.0,
        );
        cv.text_max(
            lx,
            wy + 58.0,
            &format!(
                "{} · ↑{:.0}° ↓{:.0}°",
                if wst.city.is_empty() {
                    "Here"
                } else {
                    &wst.city
                },
                wst.max,
                wst.min
            ),
            Face::Regular,
            12.0,
            p.dim,
            Align::Left,
            220.0,
        );
    } else {
        cv.icon("cloud", lx + 14.0, wy + 14.0, 22.0, p.dim);
        cv.text(
            lx + 34.0,
            wy + 14.0,
            "Weather loading…",
            Face::Regular,
            12.0,
            p.dim,
            Align::Left,
        );
    }
    // the day as a wave (the wave itself moves, above)
    cv.line(fxp + 6.0, dy, x1, dy, 4.0, p.track);
    cv.text(
        x1,
        dy - 16.0,
        &format!("{}% of today", (frac * 100.0) as i32),
        Face::Medium,
        11.0,
        p.dim,
        Align::Right,
    );

    // right: the month
    calendar(
        cv,
        ctx,
        x + w * 0.52,
        body_y + 4.0,
        w * 0.48 - 20.0,
        &tm,
        p.fg,
        p.dim,
        p.primary,
        p.ink_primary,
    );
    true
}

#[allow(clippy::too_many_arguments)]
pub fn calendar(
    cv: &mut Canvas,
    ctx: &Ctx,
    x: f32,
    y: f32,
    w: f32,
    tm: &crate::clock::Tm,
    fg: Rgba,
    dim: Rgba,
    acc: Rgba,
    ink: Rgba,
) {
    let shape = Shape::parse(&ctx.cfg.theme.shape);
    cv.text(
        x + 4.0,
        y + 10.0,
        &format!("{} {}", month(tm.mon), tm.year),
        Face::Semi,
        14.0,
        fg,
        Align::Left,
    );
    let cell = w / 7.0;
    let gy = y + 36.0;
    for (i, d) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
        cv.text(
            x + cell * (i as f32 + 0.5),
            gy,
            d,
            Face::Semi,
            11.0,
            dim,
            Align::Center,
        );
    }
    let lead = crate::clock::first_wday(tm);
    let days = crate::clock::days_in_month(tm.year, tm.mon);
    for d in 1..=days {
        let i = lead + d - 1;
        let (cx, cy) = (
            x + cell * ((i % 7) as f32 + 0.5),
            gy + 26.0 + (i / 7) as f32 * 27.0,
        );
        let today = d == tm.mday;
        if today {
            cv.shape(shape, cx, cy, 13.0, 0.0, acc);
        }
        cv.text(
            cx,
            cy,
            &d.to_string(),
            if today { Face::Bold } else { Face::Regular },
            12.0,
            if today { ink } else { fg },
            Align::Center,
        );
    }
}

/// CPU, GPU and RAM as wavy rings, the disk as a bar.
fn system(cv: &mut Canvas, ctx: &Ctx, x: f32, y: f32, w: f32, h: f32, part: Part) -> bool {
    let p = ctx.th.pal;
    let s = &ctx.st.sys;
    let mut items: Vec<(&str, f32, String)> =
        vec![("CPU", s.cpu, format!("{}%", (s.cpu * 100.0).round()))];
    if let Some(g) = s.gpu {
        items.push(("GPU", g, format!("{}%", (g * 100.0).round())));
    } else if let Some(t) = s.temp {
        items.push(("Heat", (t / 100.0).clamp(0.0, 1.0), format!("{:.0}°", t)));
    }
    items.push(("RAM", s.ram, format!("{}%", (s.ram * 100.0).round())));
    let n = items.len() as f32;
    let cell = (w - 40.0) / n;
    let phase = -crate::ui::secs(ctx.now) * 1.2;
    for (i, (label, v, txt)) in items.iter().enumerate() {
        let cx = x + 20.0 + cell * (i as f32 + 0.5);
        let cy = y + 74.0;
        let r = 50.0;
        let sweep = std::f32::consts::TAU * v.clamp(0.0, 1.0);
        if part.still() {
            cv.ring(cx, cy, r, 7.0, p.track);
            if !ctx.cfg.theme.wavy {
                cv.arc(
                    cx,
                    cy,
                    r,
                    -std::f32::consts::FRAC_PI_2,
                    sweep,
                    7.0,
                    p.primary,
                );
            }
            cv.text(cx, cy - 4.0, txt, Face::Numbers, 20.0, p.fg, Align::Center);
            cv.text(
                cx,
                cy + 18.0,
                label,
                Face::Medium,
                11.0,
                p.dim,
                Align::Center,
            );
        }
        if part.moving() && ctx.cfg.theme.wavy {
            cv.wavy_arc(
                cx,
                cy,
                r,
                2.6,
                14.0,
                phase,
                -std::f32::consts::FRAC_PI_2,
                sweep,
                7.0,
                p.primary,
            );
        }
    }
    if !part.still() {
        return ctx.cfg.theme.wavy;
    }
    // the disk
    let dy = y + h - 46.0;
    cv.icon("hard_drive", x + 34.0, dy + 6.0, 20.0, p.dim);
    let (x0, x1) = (x + 56.0, x + w - 120.0);
    cv.rrect(x0, dy + 1.0, x1 - x0, 10.0, 5.0, p.track);
    cv.rrect(
        x0,
        dy + 1.0,
        ((x1 - x0) * s.disk).max(10.0),
        10.0,
        5.0,
        p.primary,
    );
    cv.text(
        x + w - 24.0,
        dy + 6.0,
        &format!("{:.0} / {:.0} GB", s.disk_used, s.disk_total),
        Face::Medium,
        12.0,
        p.dim,
        Align::Right,
    );
    if s.ram_total > 0.0 {
        cv.text(
            x + w / 2.0,
            dy - 22.0,
            &format!("Memory {:.1} of {:.1} GB", s.ram_used, s.ram_total),
            Face::Regular,
            12.0,
            p.dim,
            Align::Center,
        );
    }
    ctx.cfg.theme.wavy
}

#[allow(clippy::too_many_arguments)]
pub fn player(
    cv: &mut Canvas,
    ctx: &Ctx,
    hits: &mut Hits,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    part: Part,
) -> bool {
    let p = ctx.th.pal;
    let m = &ctx.st.media;
    let now = ctx.now;
    let cr = 62.0;
    let (ccx, ccy) = (x + 20.0 + cr, y + h / 2.0);
    let rot = if m.playing {
        crate::ui::secs(now) * 0.25
    } else {
        0.0
    };
    let tx = x + 2.0 * cr + 42.0;
    let tw = x + w - tx - 20.0;
    let ly = y + 96.0;
    if part.moving() {
        match &m.art {
            Some(img) => cv.image_shape(img, Shape::Cookie(9), ccx, ccy, cr, rot),
            None => {
                cv.shape(Shape::Cookie(9), ccx, ccy, cr, rot, p.container);
                cv.icon("music_note", ccx, ccy, 40.0, p.ink_container);
            }
        }
        let frac = ctx.seek.unwrap_or(m.fraction(now));
        seek_line(cv, ctx, hits, tx, tx + tw, ly, frac, m.playing);
    } else {
        hits.slider(tx - 6.0, ly - 12.0, tw + 12.0, 24.0, Act::Seek);
    }
    if !part.still() {
        return m.playing;
    }
    let title = if m.title.is_empty() {
        "Nothing playing".to_string()
    } else {
        m.title.clone()
    };
    cv.text_max(
        tx,
        y + 34.0,
        &title,
        Face::Bold,
        17.0,
        p.fg,
        Align::Left,
        tw,
    );
    let sub = if m.artist.is_empty() {
        m.identity.clone()
    } else {
        m.artist.clone()
    };
    cv.text_max(
        tx,
        y + 58.0,
        &sub,
        Face::Regular,
        13.0,
        p.dim,
        Align::Left,
        tw,
    );
    cv.text(
        tx,
        ly + 20.0,
        &fmt_time(m.position(now)),
        Face::Medium,
        11.0,
        p.dim,
        Align::Left,
    );
    if m.length_us > 0 {
        cv.text(
            tx + tw,
            ly + 20.0,
            &fmt_time(m.length_us as f64 / 1e6),
            Face::Medium,
            11.0,
            p.dim,
            Align::Right,
        );
    }
    // buttons
    let by = y + h - 36.0;
    let mid = tx + tw / 2.0;
    icon_button(
        cv,
        hits,
        mid - 66.0,
        by,
        18.0,
        "skip_previous",
        p.fg,
        rgba(0.0, 0.0, 0.0, 0.0),
        p.card_hi,
        Act::Prev,
    );
    let hov = hits.add(mid - 30.0, by - 20.0, 60.0, 40.0, Act::PlayPause);
    let rr = if m.playing { 14.0 } else { 20.0 };
    cv.rrect(
        mid - 30.0,
        by - 20.0,
        60.0,
        40.0,
        rr,
        if hov {
            p.primary.lighter(1.08)
        } else {
            p.primary
        },
    );
    cv.icon(
        if m.playing { "pause" } else { "play_arrow" },
        mid,
        by,
        24.0,
        p.ink_primary,
    );
    icon_button(
        cv,
        hits,
        mid + 66.0,
        by,
        18.0,
        "skip_next",
        p.fg,
        rgba(0.0, 0.0, 0.0, 0.0),
        p.card_hi,
        Act::Next,
    );
    m.playing
}

#[allow(clippy::too_many_arguments)]
pub fn seek_line(
    cv: &mut Canvas,
    ctx: &Ctx,
    hits: &mut Hits,
    x0: f32,
    x1: f32,
    y: f32,
    frac: f32,
    playing: bool,
) {
    let p = ctx.th.pal;
    hits.slider(x0 - 6.0, y - 12.0, x1 - x0 + 12.0, 24.0, Act::Seek);
    let fx = x0 + (x1 - x0) * frac.clamp(0.0, 1.0);
    cv.line((fx + 8.0).min(x1), y, x1, y, 4.0, p.track);
    if ctx.cfg.theme.wavy && playing {
        cv.wavy_line(
            x0,
            fx,
            y,
            3.0,
            20.0,
            -crate::ui::secs(ctx.now) * 3.0,
            4.0,
            p.primary,
        );
    } else {
        cv.line(x0, y, fx, y, 4.0, p.primary);
    }
    cv.rrect(fx - 2.0, y - 10.0, 4.0, 20.0, 2.0, p.primary);
}

#[allow(clippy::too_many_arguments)]
fn notif(
    cv: &mut Canvas,
    ctx: &Ctx,
    ui: &Ui,
    hits: &mut Hits,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    part: Part,
) -> bool {
    let p = ctx.th.pal;
    let id = ui.card.notif.or(ctx.st.notifs.popup);
    let Some(n) = id.and_then(|id| ctx.st.notifs.list.iter().find(|n| n.id == id)) else {
        return false;
    };
    if part.moving() {
        // the wave that runs out while it stays
        let life = if n.timeout_ms > 0 {
            n.timeout_ms as f32 / 1000.0
        } else {
            ctx.cfg.notifications.timeout as f32
        };
        let since = ctx.st.notifs.popup_at.unwrap_or(n.at);
        let left = 1.0
            - (Instant::now()
                .saturating_duration_since(since)
                .as_secs_f32()
                / life.max(1.0))
            .clamp(0.0, 1.0);
        if n.urgency < 2 && !ui.card.hovered {
            let (x0, x1) = (x + 24.0, x + 24.0 + (w - 48.0) * left);
            cv.wavy_line(
                x0,
                x1,
                y + h - 8.0,
                1.6,
                14.0,
                -crate::ui::secs(ctx.now) * 2.0,
                2.5,
                p.primary.alpha(0.8),
            );
        }
    }
    if !part.still() {
        return n.urgency < 2 && !ui.card.hovered;
    }
    hits.add(x, y, w, h, Act::NotifOpen(n.id));
    let (ix, iy) = (x + 20.0 + 26.0, y + h / 2.0 - 4.0);
    match &n.image {
        Some(img) => cv.image_shape(img, Shape::Cookie(7), ix, iy, 26.0, 0.0),
        None => {
            cv.shape(Shape::Cookie(7), ix, iy, 26.0, 0.0, p.container);
            cv.icon("notifications", ix, iy, 24.0, p.ink_container);
        }
    }
    let tx = x + 86.0;
    let tw = w - 86.0 - 50.0;
    let ago = n.at.elapsed().as_secs();
    let when = if ago < 60 {
        "now".to_string()
    } else {
        format!("{} min", ago / 60)
    };
    let app = if n.app.is_empty() {
        "Notification".to_string()
    } else {
        n.app.clone()
    };
    cv.text_max(
        tx,
        y + 24.0,
        &format!("{app} · {when}"),
        Face::Medium,
        11.0,
        p.dim,
        Align::Left,
        tw,
    );
    cv.text_max(
        tx,
        y + 45.0,
        &n.summary,
        Face::Bold,
        15.0,
        p.fg,
        Align::Left,
        tw,
    );
    cv.text_max(
        tx,
        y + 67.0,
        &n.body.replace('\n', " "),
        Face::Regular,
        13.0,
        p.dim,
        Align::Left,
        tw,
    );
    icon_button(
        cv,
        hits,
        x + w - 28.0,
        y + 26.0,
        15.0,
        "close",
        p.dim,
        rgba(0.0, 0.0, 0.0, 0.0),
        p.card_hi,
        Act::NotifDismiss(n.id),
    );
    // actions as small pills under the close button
    let mut ax = x + w - 20.0;
    for (key, label) in n.actions.iter().filter(|a| a.0 != "default").take(2) {
        let lw = cv.measure(label, Face::Semi, 11.0) + 18.0;
        ax -= lw;
        let hov = hits.add(
            ax,
            y + h - 38.0,
            lw,
            24.0,
            Act::NotifAction(n.id, key.clone()),
        );
        cv.rrect(
            ax,
            y + h - 38.0,
            lw,
            24.0,
            12.0,
            if hov { p.primary } else { p.container },
        );
        cv.text(
            ax + lw / 2.0,
            y + h - 26.0,
            label,
            Face::Semi,
            11.0,
            if hov { p.ink_primary } else { p.ink_container },
            Align::Center,
        );
        ax -= 6.0;
    }
    n.urgency < 2 && !ui.card.hovered
}
