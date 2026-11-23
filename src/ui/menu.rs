//! A tray app's menu (from its dbusmenu), drawn by the shell next to the bar.

use crate::color::rgba;
use crate::gfx::{Align, Canvas};
use crate::shell::SHADOW;
use crate::text::Face;
use crate::ui::{Act, Ctx, Hits, Out, Ui};

pub const ROW: f32 = 34.0;
pub const SEP: f32 = 11.0;

pub fn height(entries: &[crate::svc::tray::MenuEntry]) -> f32 {
    entries.iter().map(|e| if e.separator { SEP } else { ROW }).sum::<f32>() + 16.0
}

pub fn draw(cv: &mut Canvas, ctx: &Ctx, ui: &mut Ui, hits: &mut Hits) -> Out {
    let now = ctx.now;
    let mu = &ui.menu;
    if mu.rev.gone(now) {
        return Out { anim: false, input: Some(vec![]), done: true, damage: None };
    }
    let p = ctx.th.pal;
    let k = mu.rev.value(now);
    let m = SHADOW as f32;
    let top = ctx.cfg.bar.position == "top";
    let (w, h) = (cv.w() - 2.0 * m, cv.h() - 2.0 * m);
    let off = (1.0 - k) * 10.0;
    let (x, y) = (m, m + if top { -off } else { off });
    cv.alpha = k;
    if ctx.cfg.theme.shadows {
        cv.shadow(x, y + 3.0, w, h, 20.0, 12.0, 0.6);
    }
    cv.rrect(x, y, w, h, 20.0, p.surface.alpha(p.surface.a.max(0.96)));
    cv.rrect_stroke(x, y, w, h, 20.0, 1.0, p.line);
    let mut yy = y + 8.0;
    for e in &mu.entries {
        if e.separator {
            cv.line(x + 14.0, yy + SEP / 2.0, x + w - 14.0, yy + SEP / 2.0, 1.0, p.line.alpha(0.6).lighter(2.0));
            yy += SEP;
            continue;
        }
        let ix = x + 8.0 + e.depth as f32 * 14.0;
        let clickable = e.enabled && !e.submenu;
        let hov = clickable && hits.add(x + 6.0, yy, w - 12.0, ROW, Act::MenuPick(e.id));
        if hov {
            cv.rrect(x + 6.0, yy + 2.0, w - 12.0, ROW - 4.0, 12.0, p.card_hi);
        }
        let fg = if !e.enabled { p.dim.alpha(0.6) } else if e.submenu { p.dim } else { p.fg };
        let mut tx = ix + 10.0;
        if let Some(on) = e.check {
            if on {
                cv.icon("check", ix + 16.0, yy + ROW / 2.0, 16.0, p.primary);
            }
            tx = ix + 32.0;
        }
        let face = if e.submenu { Face::Semi } else { Face::Medium };
        cv.text_max(tx, yy + ROW / 2.0, &e.label, face, if e.submenu { 11.5 } else { 13.0 }, fg, Align::Left, x + w - tx - 12.0);
        yy += ROW;
    }
    let _ = rgba(0.0, 0.0, 0.0, 0.0);
    cv.alpha = 1.0;
    Out { anim: mu.rev.animating(now), input: Some(vec![(x, y, w, h)]), done: false, damage: None }
}
