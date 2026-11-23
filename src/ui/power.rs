//! The power menu: lock, sleep, log out, restart, shut down.

use crate::color::{Rgba, rgba};
use crate::gfx::{Align, Canvas, Shape};
use crate::shell::SHADOW;
use crate::text::Face;
use crate::ui::{Act, Ctx, Hits, Out, PowerAct, Ui};

pub const ACTS: [(PowerAct, &str, &str); 5] = [
    (PowerAct::Lock, "lock", "Lock"),
    (PowerAct::Suspend, "bedtime", "Sleep"),
    (PowerAct::Logout, "logout", "Log out"),
    (PowerAct::Reboot, "restart_alt", "Restart"),
    (PowerAct::Shutdown, "power_settings_new", "Shut down"),
];

pub fn draw(cv: &mut Canvas, ctx: &Ctx, ui: &mut Ui, hits: &mut Hits) -> Out {
    let now = ctx.now;
    if ui.power.gone(now) {
        return Out { anim: false, input: Some(vec![]), done: true, damage: None };
    }
    let p = ctx.th.pal;
    let k = ui.power.value(now);
    let m = SHADOW as f32;
    let (x, y, w, h) = (m, m + (1.0 - k) * 14.0, cv.w() - 2.0 * m, cv.h() - 2.0 * m);
    cv.alpha = k;
    if ctx.cfg.theme.shadows {
        cv.shadow(x, y + 4.0, w, h, 32.0, 18.0, 0.7);
    }
    cv.rrect(x, y, w, h, 32.0, p.surface);
    cv.rrect_stroke(x, y, w, h, 32.0, 1.0, p.line);
    let name = if ctx.st.user.real.is_empty() { ctx.st.user.name.clone() } else { ctx.st.user.real.clone() };
    cv.text(x + 28.0, y + 34.0, &format!("Goodbye for now, {name}"), Face::Semi, 15.0, p.fg, Align::Left);
    cv.text(x + w - 28.0, y + 34.0, "Esc to stay", Face::Regular, 12.0, p.dim, Align::Right);
    let n = ACTS.len() as f32;
    let cell = (w - 40.0) / n;
    let red = Rgba::hex("#f2b8b5").unwrap();
    for (i, (pa, icon, label)) in ACTS.iter().enumerate() {
        let cx = x + 20.0 + cell * (i as f32 + 0.5);
        let cy = y + 100.0;
        let sel = ui.power_sel == i;
        let hov = hits.add(cx - cell / 2.0 + 4.0, cy - 44.0, cell - 8.0, 104.0, Act::Power(*pa));
        if hov {
            ui.power_sel = i;
        }
        let danger = *pa == PowerAct::Shutdown;
        let (bg, fg) = if sel || hov {
            if danger { (red, rgba(0.38, 0.08, 0.07, 1.0)) } else { (p.primary, p.ink_primary) }
        } else {
            (p.card, if danger { red } else { p.fg })
        };
        let shape = if sel || hov { Shape::parse(&ctx.cfg.theme.shape) } else { Shape::Circle };
        cv.shape(shape, cx, cy, 34.0, 0.0, bg);
        cv.icon(icon, cx, cy, 28.0, fg);
        cv.text(cx, cy + 52.0, label, Face::Medium, 13.0, if sel { p.fg } else { p.dim }, Align::Center);
    }
    cv.alpha = 1.0;
    Out { anim: ui.power.animating(now), input: None, done: false, damage: None }
}
