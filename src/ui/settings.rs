//! The Settings window: a navigation rail, the page title, and rows of switches, choices and sliders
//! (Material 3, like the original's Settings app). The rows scroll inside their own clipped area.

use tiny_skia::{Pixmap, PixmapPaint, Transform};

use crate::color::{Rgba, rgba};
use crate::gfx::{Align, Canvas, Shape};
use crate::settings::{Ctl, PAGES, Row};
use crate::shell::SHADOW;
use crate::text::Face;
use crate::theme::SCHEMES;
use crate::ui::{Act, Ctx, Hits, Out, Ui, icon_button};

const SHAPES: &[(&str, &str)] = &[
    ("verysunny", "Very sunny"),
    ("sunny", "Sunny"),
    ("cookie", "Cookie"),
    ("cookie4", "Cookie 4"),
    ("cookie6", "Cookie 6"),
    ("flower", "Flower"),
    ("clover", "Clover"),
    ("pentagon", "Pentagon"),
    ("hexagon", "Hexagon"),
    ("square", "Square"),
    ("circle", "Circle"),
];

fn row_height(r: &Row) -> f32 {
    match r.ctl {
        _ if r.key.is_empty() && matches!(r.ctl, Ctl::Info) => 58.0,
        Ctl::Scheme => 172.0,
        Ctl::Shape => 112.0,
        _ => 70.0,
    }
}

/// The content height of a page (for scrolling).
pub fn page_height(page: usize) -> f32 {
    PAGES[page.min(PAGES.len() - 1)].rows.iter().map(|r| row_height(r) + 8.0).sum::<f32>() + 12.0
}

fn info_text(ctx: &Ctx, key: &str) -> String {
    match key {
        "about.version" => format!("dshell {}", env!("CARGO_PKG_VERSION")),
        "about.wm" => match ctx.st.wm.kind {
            crate::state::WmKind::Hyprland => "Hyprland".into(),
            crate::state::WmKind::Drift => "driftwm".into(),
            crate::state::WmKind::None => "none found".into(),
        },
        "about.memory" => {
            let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
            let get = |k: &str| {
                s.lines()
                    .find(|l| l.starts_with(k))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|v| v.parse::<f32>().ok())
                    .unwrap_or(0.0)
                    / 1024.0
            };
            format!("{:.1} MB of its own · {:.1} MB in all", get("RssAnon:"), get("VmRSS:"))
        }
        _ => String::new(),
    }
}

pub fn draw(cv: &mut Canvas, ctx: &Ctx, ui: &mut Ui, hits: &mut Hits) -> Out {
    let now = ctx.now;
    let su = &ui.settings;
    if su.rev.gone(now) {
        return Out { anim: false, input: Some(vec![]), done: true, damage: None };
    }
    let p = ctx.th.pal;
    let k = su.rev.value(now);
    let m = SHADOW as f32;
    let (w, h) = (cv.w() - 2.0 * m, cv.h() - 2.0 * m);
    let (x, y) = (m, m + (1.0 - k) * 20.0);
    cv.alpha = k;
    if ctx.cfg.theme.shadows {
        cv.shadow(x, y + 4.0, w, h, 30.0, 18.0, 0.7);
    }
    let solid = p.surface.alpha(p.surface.a.max(0.94));
    cv.rrect(x, y, w, h, 30.0, solid);
    cv.rrect_stroke(x, y, w, h, 30.0, 1.5, p.primary.alpha(0.35));

    // the navigation rail
    let page = (su.page as usize).min(PAGES.len() - 1);
    let rail_w = 96.0;
    // the user at the top of the rail
    let (ax, ay) = (x + rail_w / 2.0, y + 44.0);
    match &ctx.st.user.avatar {
        Some(img) => cv.image_shape(img, Shape::parse(&ctx.cfg.theme.shape), ax, ay, 24.0, 0.0),
        None => {
            cv.shape(Shape::parse(&ctx.cfg.theme.shape), ax, ay, 24.0, 0.0, p.container);
            let l: String = ctx.st.user.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
            cv.text(ax, ay, &l, Face::Bold, 18.0, p.ink_container, Align::Center);
        }
    }
    let item_h = 58.0;
    for (i, pg) in PAGES.iter().enumerate() {
        let iy = y + 92.0 + i as f32 * item_h;
        let on = i == page;
        let hov = hits.add(x + 8.0, iy, rail_w - 16.0, item_h - 4.0, Act::SettingsPage(i as u8));
        if on {
            cv.rrect(ax - 28.0, iy + 4.0, 56.0, 30.0, 15.0, p.container);
        } else if hov {
            cv.rrect(ax - 28.0, iy + 4.0, 56.0, 30.0, 15.0, rgba(1.0, 1.0, 1.0, 0.06));
        }
        cv.icon(pg.icon, ax, iy + 19.0, 20.0, if on { p.ink_container } else { p.dim });
        cv.text(ax, iy + 46.0, pg.name, if on { Face::Semi } else { Face::Medium }, 11.0, if on { p.fg } else { p.dim }, Align::Center);
    }

    // header
    let pg = &PAGES[page];
    let cx0 = x + rail_w + 8.0;
    let cw = w - rail_w - 32.0;
    cv.text(cx0, y + 34.0, &format!("DARKSHADOW'S SHELL  ·  {}", pg.sub.to_uppercase()), Face::Semi, 11.0, p.primary, Align::Left);
    cv.text(cx0, y + 66.0, pg.name, Face::Semi, 26.0, p.fg, Align::Left);
    icon_button(cv, hits, x + w - 40.0, y + 40.0, 20.0, "close", p.fg, p.card, p.card_hi, Act::CloseAll);

    // the rows, drawn into their own pixmap so they are clipped at the edges while scrolling
    let (vx, vy, vw, vh) = (cx0 - 8.0, y + 96.0, cw + 16.0, h - 96.0 - 14.0);
    let max_scroll = (page_height(page) - vh).max(0.0);
    let scroll = ui.settings.scroll.clamp(0.0, max_scroll);
    ui.settings.scroll = scroll;
    let s = cv.s;
    let (pw, ph) = ((vw * s).ceil().max(1.0) as u32, (vh * s).ceil().max(1.0) as u32);
    let hit_start = hits.list.len();
    if let Some(mut sub) = Pixmap::new(pw, ph) {
        {
            let mut sc = Canvas::new(sub.as_mut(), s, &mut *cv.g);
            sc.ox = -vx;
            sc.oy = -vy;
            sc.alpha = 1.0;
            let mut ry = vy + 4.0 - scroll;
            for r in pg.rows {
                let rh = row_height(r);
                if ry + rh >= vy - 2.0 && ry <= vy + vh + 2.0 {
                    draw_row(&mut sc, ctx, hits, r, cx0, ry, cw, rh);
                }
                ry += rh + 8.0;
            }
        }
        let paint = PixmapPaint { opacity: k, ..Default::default() };
        let (dx, dy) = (((vx + cv.ox) * s).round() as i32, ((vy + cv.oy) * s).round() as i32);
        cv.pm.draw_pixmap(dx, dy, sub.as_ref(), &paint, Transform::identity(), None);
    }
    // hit areas outside the visible part are cut away
    let mut keep = Vec::with_capacity(hits.list.len());
    for (i, mut ht) in hits.list.drain(..).enumerate() {
        if i >= hit_start {
            let top = ht.y.max(vy);
            let bot = (ht.y + ht.h).min(vy + vh);
            if bot <= top {
                continue;
            }
            ht.y = top;
            ht.h = bot - top;
        }
        keep.push(ht);
    }
    hits.list = keep;
    // the scroll bar
    if max_scroll > 0.0 {
        let th = (vh * vh / page_height(page)).max(30.0);
        let ty = vy + (vh - th) * (scroll / max_scroll);
        cv.rrect(x + w - 10.0, ty, 4.0, th, 2.0, p.dim.alpha(0.35));
    }
    cv.alpha = 1.0;
    Out { anim: ui.settings.rev.animating(now), input: None, done: false, damage: None }
}

#[allow(clippy::too_many_arguments)]
fn draw_row(cv: &mut Canvas, ctx: &Ctx, hits: &mut Hits, r: &Row, x: f32, y: f32, w: f32, h: f32) {
    let p = ctx.th.pal;
    // a section title
    if r.key.is_empty() && matches!(r.ctl, Ctl::Info) {
        cv.text(x, y + 26.0, r.title, Face::Bold, 16.0, p.fg, Align::Left);
        if !r.desc.is_empty() {
            cv.text_max(x, y + 46.0, r.desc, Face::Regular, 12.0, p.dim, Align::Left, w);
        }
        return;
    }
    cv.rrect(x, y, w, h, 16.0, p.card);
    let ctl_w = match &r.ctl {
        Ctl::Choice(opts) => opts.iter().map(|o| cv.measure(o.1, Face::Medium, 13.0) + 40.0).sum::<f32>() + 8.0,
        Ctl::Slider { .. } => 270.0,
        Ctl::Button(label, _) => cv.measure(label, Face::Semi, 13.0) + 40.0,
        Ctl::Info if !r.key.is_empty() => 360.0,
        _ => 70.0,
    };
    let full = matches!(r.ctl, Ctl::Scheme | Ctl::Shape);
    let text_w = if full { w - 36.0 } else { (w - ctl_w - 48.0).max(120.0) };
    let ty = if full { y + 26.0 } else { y + h / 2.0 };
    if r.desc.is_empty() && !full {
        cv.text_max(x + 18.0, ty, r.title, Face::Medium, 14.0, p.fg, Align::Left, text_w);
    } else {
        cv.text_max(x + 18.0, ty - 9.0, r.title, Face::Medium, 14.0, p.fg, Align::Left, text_w);
        if !r.desc.is_empty() {
            cv.text_max(x + 18.0, ty + 11.0, r.desc, Face::Regular, 12.0, p.dim, Align::Left, text_w);
        } else if full {
            cv.text_max(x + 18.0, ty + 11.0, "", Face::Regular, 12.0, p.dim, Align::Left, text_w);
        }
    }
    let val = crate::settings::value(ctx.cfg, r.key);
    let right = x + w - 18.0;
    let cy = y + h / 2.0;
    match &r.ctl {
        Ctl::Switch => {
            let on = val.as_ref().and_then(|v| v.as_bool()).unwrap_or(false);
            let (sw, sh) = (52.0, 32.0);
            let sx = right - sw;
            let hov = hits.add(x, y, w, h, Act::CfgToggle(r.key));
            switch(cv, ctx, sx, cy - sh / 2.0, on, hov);
        }
        Ctl::Choice(opts) => {
            let cur = val.as_ref().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            let mut bx = right - ctl_w + 8.0;
            for (v, label) in opts.iter() {
                let bw = cv.measure(label, Face::Medium, 13.0) + 32.0 + if *v == cur { 8.0 } else { 0.0 };
                let on = *v == cur;
                let hov = hits.add(bx, cy - 18.0, bw, 36.0, Act::CfgSet(r.key, v.to_string()));
                let bg = if on { p.primary } else if hov { p.card_hi } else { p.surface.alpha(0.6) };
                cv.rrect(bx, cy - 18.0, bw, 36.0, 18.0, bg);
                if on {
                    cv.icon("check", bx + 18.0, cy, 16.0, p.ink_primary);
                    cv.text(bx + 30.0, cy, label, Face::Semi, 13.0, p.ink_primary, Align::Left);
                } else {
                    cv.text(bx + bw / 2.0, cy, label, Face::Medium, 13.0, p.fg, Align::Center);
                }
                bx += bw + 4.0;
            }
        }
        Ctl::Slider { min, max, int, .. } => {
            let v = val.as_ref().and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64))).unwrap_or(*min);
            let f = ((v - min) / (max - min)).clamp(0.0, 1.0) as f32;
            let txt = if *int { format!("{}", v.round() as i64) } else { format!("{v:.2}") };
            cv.text(right - 210.0, cy, &txt, Face::Medium, 13.0, p.dim, Align::Right);
            let (x0, x1) = (right - 196.0, right - 4.0);
            let active = hits.slider(x0 - 8.0, y, x1 - x0 + 16.0, h, Act::CfgSlide(r.key));
            let fx = x0 + (x1 - x0) * f;
            cv.rrect(x0, cy - 8.0, (fx - x0 - 4.0).max(8.0), 16.0, 8.0, p.primary);
            cv.rrect(fx + 4.0, cy - 8.0, (x1 - fx - 4.0).max(0.0), 16.0, 8.0, p.card_hi);
            cv.circle(x1 - 6.0, cy, 2.5, p.primary);
            cv.rrect(fx - 2.0, cy - 14.0, 4.0, 28.0, 2.0, if active { p.fg } else { p.primary });
        }
        Ctl::Scheme => {
            let cur = val.as_ref().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            let per_row = ((w - 36.0) / 62.0).floor().max(1.0) as usize;
            for (i, (key, name, hex)) in SCHEMES.iter().enumerate() {
                let (col, row) = (i % per_row, i / per_row);
                let (cx, ccy) = (x + 18.0 + 22.0 + col as f32 * 62.0, y + 66.0 + row as f32 * 60.0);
                let color = if hex.is_empty() {
                    if *key == "grey" {
                        let l = ctx.th.accent.luma().clamp(0.5, 0.75);
                        rgba(l, l, l, 1.0)
                    } else {
                        ctx.st.wall.accent.unwrap_or(ctx.th.accent)
                    }
                } else {
                    Rgba::hex(hex).unwrap_or(p.primary)
                };
                let on = *key == cur;
                let hov = hits.add(cx - 26.0, ccy - 24.0, 52.0, 58.0, Act::CfgSet(r.key, key.to_string()));
                if on {
                    cv.shape(Shape::parse(&ctx.cfg.theme.shape), cx, ccy, 22.0, 0.0, color);
                    cv.icon("check", cx, ccy, 18.0, rgba(0.08, 0.08, 0.1, 0.9));
                } else {
                    cv.circle(cx, ccy, if hov { 19.0 } else { 17.0 }, color);
                    if *key == "auto" {
                        cv.icon("wallpaper", cx, ccy, 15.0, rgba(0.08, 0.08, 0.1, 0.8));
                    }
                }
                cv.text_max(cx, ccy + 31.0, name, if on { Face::Semi } else { Face::Regular }, 10.5, if on { p.fg } else { p.dim }, Align::Center, 58.0);
            }
        }
        Ctl::Shape => {
            let cur = val.as_ref().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            for (i, (key, name)) in SHAPES.iter().enumerate() {
                let (cx, ccy) = (x + 18.0 + 26.0 + i as f32 * 70.0, y + 70.0);
                if cx > x + w - 20.0 {
                    break;
                }
                let on = *key == cur;
                let hov = hits.add(cx - 30.0, ccy - 26.0, 60.0, 62.0, Act::CfgSet(r.key, key.to_string()));
                let col = if on { p.primary } else if hov { p.card_hi.lighter(1.3) } else { p.card_hi };
                cv.shape(Shape::parse(key), cx, ccy, 20.0, 0.0, col);
                cv.text_max(cx, ccy + 32.0, name, if on { Face::Semi } else { Face::Regular }, 10.5, if on { p.fg } else { p.dim }, Align::Center, 66.0);
            }
        }
        Ctl::Button(label, act) => {
            let bw = cv.measure(label, Face::Semi, 13.0) + 32.0;
            let bx = right - bw;
            let hov = hits.add(bx, cy - 18.0, bw, 36.0, act());
            cv.rrect(bx, cy - 18.0, bw, 36.0, 18.0, if hov { p.primary.lighter(1.08) } else { p.primary });
            cv.text(bx + bw / 2.0, cy, label, Face::Semi, 13.0, p.ink_primary, Align::Center);
        }
        Ctl::Info => {
            let t = info_text(ctx, r.key);
            cv.text_max(right, cy, &t, Face::Medium, 13.0, p.dim, Align::Right, 360.0);
        }
    }
}

/// A Material 3 switch.
fn switch(cv: &mut Canvas, ctx: &Ctx, x: f32, y: f32, on: bool, hov: bool) {
    let p = ctx.th.pal;
    let (w, h) = (52.0, 32.0);
    if on {
        cv.rrect(x, y, w, h, h / 2.0, p.primary);
        cv.circle(x + w - h / 2.0, y + h / 2.0, if hov { 13.0 } else { 12.0 }, p.ink_primary);
        cv.icon("check", x + w - h / 2.0, y + h / 2.0, 14.0, p.primary);
    } else {
        cv.rrect(x, y, w, h, h / 2.0, p.surface.alpha(0.6));
        cv.rrect_stroke(x, y, w, h, h / 2.0, 2.0, p.dim.alpha(0.7));
        cv.circle(x + h / 2.0, y + h / 2.0, if hov { 10.0 } else { 8.0 }, p.dim);
        cv.icon("close", x + h / 2.0, y + h / 2.0, 11.0, p.surface.alpha(1.0));
    }
}
