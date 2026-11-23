//! The dock: pinned apps and the ones that are open, with a dot under the running ones.
//! Click: open it (or bring its window forward); right click: pin or unpin.

use crate::color::rgba;
use crate::gfx::{Align, Canvas, Shape};
use crate::text::Face;
use crate::ui::{Act, Ctx, Hits, Out, Ui};

pub struct Item {
    /// desktop id, or "class:<window class>" for an open app without a desktop file
    pub key: String,
    pub name: String,
    pub icon: String,
    pub running: u32,
    pub focused: bool,
    pub pinned: bool,
}

pub fn items(ctx: &Ctx) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    let running_of = |idx: usize| -> (u32, bool) {
        ctx.st
            .wm
            .apps
            .iter()
            .fold(
                (0, false),
                |acc, (class, n, f)| match crate::svc::apps::for_window(ctx.apps, class) {
                    Some((i, _)) if i == idx => (acc.0 + n, acc.1 || *f),
                    _ => acc,
                },
            )
    };
    for id in &ctx.cfg.dock.pinned {
        if let Some((i, e)) = ctx.apps.iter().enumerate().find(|(_, e)| &e.id == id) {
            let (n, f) = running_of(i);
            out.push(Item {
                key: e.id.clone(),
                name: e.name.clone(),
                icon: e.icon.clone(),
                running: n,
                focused: f,
                pinned: true,
            });
        }
    }
    if ctx.cfg.dock.running {
        for (class, n, f) in &ctx.st.wm.apps {
            match crate::svc::apps::for_window(ctx.apps, class) {
                Some((_, e)) => {
                    if !out.iter().any(|x| x.key == e.id) {
                        out.push(Item {
                            key: e.id.clone(),
                            name: e.name.clone(),
                            icon: e.icon.clone(),
                            running: *n,
                            focused: *f,
                            pinned: false,
                        });
                    }
                }
                None => out.push(Item {
                    key: format!("class:{class}"),
                    name: class.clone(),
                    icon: String::new(),
                    running: *n,
                    focused: *f,
                    pinned: false,
                }),
            }
        }
    }
    out
}

pub fn draw(cv: &mut Canvas, ctx: &Ctx, ui: &mut Ui, hits: &mut Hits) -> Out {
    let p = ctx.th.pal;
    let list = items(ctx);
    let s = ctx.cfg.dock.size.clamp(28, 72) as f32;
    let slot = s + 14.0;
    let (w, h) = (cv.w(), cv.h());
    let pw = (list.len().max(1) as f32 * slot + 16.0).min(w - 8.0);
    let ph = s + 20.0;
    let x0 = ((w - pw) / 2.0).round();
    let y0 = h - ph - 4.0;
    let alpha = if ctx.cfg.theme.flat || ctx.st.toggles.gaming {
        1.0
    } else {
        ctx.cfg.bar.opacity.clamp(0.2, 1.0)
    };
    if ctx.cfg.theme.shadows {
        cv.shadow(x0, y0 + 2.0, pw, ph, 22.0, 10.0, 0.55);
    }
    cv.rrect(x0, y0, pw, ph, 22.0, p.surface.alpha(alpha));
    cv.rrect_stroke(x0, y0, pw, ph, 22.0, 1.0, rgba(1.0, 1.0, 1.0, 0.06));
    if list.is_empty() {
        cv.text(
            x0 + pw / 2.0,
            y0 + ph / 2.0,
            "Pin apps from the launcher",
            Face::Medium,
            12.0,
            p.dim,
            Align::Center,
        );
    }
    let mut tooltip: Option<(f32, String)> = None;
    for (i, it) in list.iter().enumerate() {
        let cx = x0 + 8.0 + slot * (i as f32 + 0.5);
        let cy = y0 + ph / 2.0 - 2.0;
        // a thin line between the pinned apps and the other open ones
        if i > 0 && list[i - 1].pinned && !it.pinned {
            cv.line(
                cx - slot / 2.0,
                y0 + 12.0,
                cx - slot / 2.0,
                y0 + ph - 12.0,
                1.0,
                p.dim.alpha(0.35),
            );
        }
        let hov = hits.add(
            cx - slot / 2.0,
            y0,
            slot,
            ph,
            Act::DockClick(it.key.clone()),
        );
        if !it.key.starts_with("class:") {
            hits.with_right(Act::DockPin(it.key.clone()));
        }
        let size = if hov { s + 4.0 } else { s };
        if hov {
            cv.rrect(
                cx - slot / 2.0 + 2.0,
                y0 + 4.0,
                slot - 4.0,
                ph - 8.0,
                16.0,
                p.card_hi,
            );
            tooltip = Some((cx, it.name.clone()));
        }
        match ctx.app_icons.get(&it.icon) {
            Some(Some(img)) => cv.image_rrect(
                img,
                cx - size / 2.0,
                cy - size / 2.0,
                size,
                size,
                size * 0.22,
            ),
            other => {
                if other.is_none() && !it.icon.is_empty() {
                    ui.want_icons.push(it.icon.clone());
                }
                cv.shape(Shape::Cookie(7), cx, cy, size / 2.0, 0.0, p.container);
                let l: String = it
                    .name
                    .chars()
                    .next()
                    .map(|c| c.to_uppercase().collect())
                    .unwrap_or_default();
                cv.text(
                    cx,
                    cy,
                    &l,
                    Face::Bold,
                    size * 0.4,
                    p.ink_container,
                    Align::Center,
                );
            }
        }
        // running: a dot; focused: a short pill
        if it.running > 0 {
            let dy = y0 + ph - 6.0;
            if it.focused {
                cv.rrect(cx - 8.0, dy - 2.0, 16.0, 4.0, 2.0, p.primary);
            } else {
                cv.circle(cx, dy, 2.2, p.dim);
            }
        }
    }
    if let Some((tx, name)) = tooltip {
        let tw = cv.measure(&name, Face::Medium, 12.0) + 20.0;
        let tx = (tx - tw / 2.0).clamp(4.0, w - tw - 4.0);
        cv.rrect(tx, y0 - 34.0, tw, 26.0, 13.0, p.surface.alpha(0.95));
        cv.text(
            tx + tw / 2.0,
            y0 - 21.0,
            &name,
            Face::Medium,
            12.0,
            p.fg,
            Align::Center,
        );
    }
    Out {
        anim: false,
        input: Some(vec![(x0, y0, pw, ph)]),
        done: false,
        damage: None,
    }
}
