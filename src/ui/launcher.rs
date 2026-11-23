//! The DarkShadow launcher (ALT + Space): a search field and a list, Material 3, keyboard first.

use crate::color::rgba;
use crate::gfx::{Align, Canvas, Shape};
use crate::shell::SHADOW;
use crate::text::Face;
use crate::ui::{Act, Ctx, Hits, Out, Ui};

pub fn draw(cv: &mut Canvas, ctx: &Ctx, ui: &mut Ui, hits: &mut Hits) -> Out {
    let now = ctx.now;
    let l = &ui.launcher;
    if l.rev.gone(now) {
        return Out { anim: false, input: Some(vec![]), done: true, damage: None };
    }
    let p = ctx.th.pal;
    let k = l.rev.value(now);
    let m = SHADOW as f32;
    let (w, h) = (cv.w() - 2.0 * m, cv.h() - 2.0 * m);
    // rises a little and scales in while it fades in
    let dy = (1.0 - k) * 18.0;
    let (x, y) = (m, m + dy);
    cv.alpha = k;
    if ctx.cfg.theme.shadows {
        cv.shadow(x, y + 4.0, w, h, 28.0, 18.0, 0.7);
    }
    cv.rrect(x, y, w, h, 28.0, p.surface);
    cv.rrect_stroke(x, y, w, h, 28.0, 1.0, p.line);

    // the search field
    let (fx, fy, fw, fh) = (x + 14.0, y + 14.0, w - 28.0, 50.0);
    cv.rrect(fx, fy, fw, fh, fh / 2.0, p.card);
    cv.icon(if l.clip_mode { "edit" } else { "search" }, fx + 26.0, fy + fh / 2.0, 22.0, p.primary);
    let q = &l.query;
    if q.is_empty() {
        cv.rrect(fx + 50.0, fy + 14.0, 2.0, fh - 28.0, 1.0, p.primary);
        let hint = if l.clip_mode { "Search the clipboard" } else { "Search apps, or type a command" };
        cv.text(fx + 58.0, fy + fh / 2.0, hint, Face::Regular, 15.0, p.dim, Align::Left);
    } else {
        let tw = cv.text_max(fx + 50.0, fy + fh / 2.0, q, Face::Medium, 15.0, p.fg, Align::Left, fw - 110.0);
        cv.rrect(fx + 51.0 + tw, fy + 14.0, 2.0, fh - 28.0, 1.0, p.primary);
    }
    let count = format!("{}", l.results.len());
    cv.text(fx + fw - 22.0, fy + fh / 2.0, &count, Face::Medium, 12.0, p.dim, Align::Right);

    // the list
    let rows = ctx.cfg.launcher.rows.clamp(3, 12) as usize;
    let row_h = 56.0;
    let ly = fy + fh + 10.0;
    if l.results.is_empty() {
        cv.shape(Shape::Cookie(9), x + w / 2.0, ly + 80.0, 34.0, 0.0, p.container);
        cv.icon("search", x + w / 2.0, ly + 80.0, 30.0, p.ink_container);
        let msg = if l.clip_mode {
            "Nothing copied yet".to_string()
        } else if q.is_empty() {
            "No apps found".to_string()
        } else {
            format!("Press Enter to run “{q}”")
        };
        cv.text(x + w / 2.0, ly + 140.0, &msg, Face::Medium, 14.0, p.dim, Align::Center);
    }
    if l.clip_mode {
        for (row, &idx) in l.results.iter().enumerate().skip(l.top).take(rows) {
            let Some((_, shown)) = l.clip.get(idx) else { continue };
            let ry = ly + (row - l.top) as f32 * row_h;
            let sel = row == l.sel;
            let hov = hits.add(x + 10.0, ry, w - 20.0, row_h - 4.0, Act::Launch(idx));
            if sel {
                cv.rrect(x + 10.0, ry, w - 20.0, row_h - 4.0, 18.0, p.container);
            } else if hov {
                cv.rrect(x + 10.0, ry, w - 20.0, row_h - 4.0, 18.0, rgba(1.0, 1.0, 1.0, 0.05));
            }
            let icy = ry + (row_h - 4.0) / 2.0;
            let image = shown.contains("binary data");
            cv.shape(Shape::Cookie(7), x + 42.0, icy, 17.0, 0.0, p.card_hi);
            cv.icon(if image { "image" } else { "edit" }, x + 42.0, icy, 18.0, if sel { p.ink_container } else { p.fg });
            let text = if image { "Image".to_string() } else { shown.replace(['\n', '\r'], " ") };
            cv.text_max(x + 72.0, icy, &text, Face::Medium, 13.5, if sel { p.ink_container } else { p.fg }, Align::Left, w - 110.0);
        }
    }
    for (row, &idx) in l.results.iter().enumerate().skip(l.top).take(if l.clip_mode { 0 } else { rows }) {
        let Some(e) = ctx.apps.get(idx) else { continue };
        let ry = ly + (row - l.top) as f32 * row_h;
        let sel = row == l.sel;
        let hov = hits.add(x + 10.0, ry, w - 20.0, row_h - 4.0, Act::Launch(idx));
        hits.with_right(Act::DockPin(e.id.clone()));
        if sel {
            cv.rrect(x + 10.0, ry, w - 20.0, row_h - 4.0, 18.0, p.container);
        } else if hov {
            cv.rrect(x + 10.0, ry, w - 20.0, row_h - 4.0, 18.0, rgba(1.0, 1.0, 1.0, 0.05));
        }
        let (icx, icy) = (x + 42.0, ry + (row_h - 4.0) / 2.0);
        match ctx.app_icons.get(&e.icon) {
            Some(Some(img)) => {
                let s = 34.0;
                cv.image_rrect(img, icx - s / 2.0, icy - s / 2.0, s, s, 8.0);
            }
            Some(None) => letter_icon(cv, ctx, &e.name, icx, icy),
            None => {
                if !e.icon.is_empty() {
                    ui.want_icons.push(e.icon.clone());
                }
                letter_icon(cv, ctx, &e.name, icx, icy);
            }
        }
        let tx = x + 72.0;
        let tw = w - 72.0 - 30.0;
        let sub = if !e.comment.is_empty() { &e.comment } else { &e.generic };
        let fg = if sel { p.ink_container } else { p.fg };
        if sub.is_empty() {
            cv.text_max(tx, icy, &e.name, Face::Semi, 14.0, fg, Align::Left, tw);
        } else {
            cv.text_max(tx, icy - 9.0, &e.name, Face::Semi, 14.0, fg, Align::Left, tw);
            cv.text_max(tx, icy + 10.0, sub, Face::Regular, 12.0, if sel { p.ink_container.alpha(0.75) } else { p.dim }, Align::Left, tw);
        }
        if sel {
            cv.icon("chevron_right", x + w - 34.0, icy, 20.0, p.ink_container);
        }
    }
    // a thin scroll hint
    let n = l.results.len();
    if n > rows {
        let track = rows as f32 * row_h - 8.0;
        let th = (track * rows as f32 / n as f32).max(24.0);
        let ty = ly + (track - th) * (l.top as f32 / (n - rows) as f32);
        cv.rrect(x + w - 8.0, ty, 3.0, th, 1.5, p.dim.alpha(0.4));
    }
    cv.alpha = 1.0;
    let anim = l.rev.animating(now);
    Out { anim, input: None, done: false, damage: None }
}

/// No icon: the first letter in your shape.
fn letter_icon(cv: &mut Canvas, ctx: &Ctx, name: &str, cx: f32, cy: f32) {
    let p = ctx.th.pal;
    cv.shape(Shape::Cookie(7), cx, cy, 17.0, 0.0, p.container);
    let l: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    cv.text(cx, cy, &l, Face::Bold, 14.0, p.ink_container, Align::Center);
}
