//! The wallpaper surface: the picture is copied into the buffer once (a fade when it changes), then the
//! decoded pixels are dropped. The compositor keeps showing the last buffer.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use crate::app::App;
use crate::gfx::Canvas;
use crate::svc::images::WallPixels;
use crate::ui::{Ctx, Kind, Out, Ui};

const FADE_MS: u64 = 700;

#[derive(Default)]
pub struct WallUi {
    cur: Option<Rc<WallPixels>>,
    prev: Option<Rc<WallPixels>>,
    since: Option<Instant>,
    drawn: bool,
    /// a real picture was shown before (a change lights up instead of popping in)
    had: bool,
    /// a decode for this surface's size was asked for
    requested: bool,
}

pub type Walls = HashMap<u32, WallUi>;

pub fn set(app: &mut App, id: u32, px: WallPixels) {
    let fade = app.cfg.wallpaper.transition == "fade" && !app.cfg.performance.gaming;
    let w = app.ui.walls.entry(id).or_default();
    let had = w.drawn;
    // the old picture fades out when it is still in memory
    w.prev = if fade && had { w.cur.take() } else { None };
    if !fade {
        w.had = false;
    }
    w.cur = Some(Rc::new(px));
    w.since = Some(Instant::now());
    w.drawn = false;
    app.mark_kind(Kind::Wall(id));
}

fn blit(dst: &mut [u8], src: &WallPixels, k: u32) {
    let n = src.rgba.len().min(dst.len());
    let dst = &mut dst[..n];
    if k >= 255 {
        dst.copy_from_slice(&src.rgba[..n]);
        return;
    }
    let inv = 255 - k;
    for (d, s) in dst.iter_mut().zip(src.rgba.iter()) {
        *d = ((*s as u32 * k + *d as u32 * inv + 127) / 255) as u8;
    }
}

pub fn draw(cv: &mut Canvas, ctx: &Ctx, ui: &mut Ui, id: u32) -> Out {
    let (bw, bh) = (cv.pm.width(), cv.pm.height());
    let redecode = &mut ui.wall_redecode;
    let w = ui.walls.entry(id).or_default();
    let Some(cur) = w.cur.clone() else {
        // nothing decoded (yet, or dropped): a quiet gradient in the theme's color
        let a = ctx.th.accent;
        let (h, _, _) = a.hsl();
        cv.rrect_vgrad(
            0.0,
            0.0,
            cv.w(),
            cv.h(),
            0.0,
            crate::color::Rgba::hsla(h.max(0.0), 0.25, 0.16, 1.0),
            crate::color::Rgba::hsla(h.max(0.0), 0.2, 0.05, 1.0),
            1.0,
        );
        if !ctx.st.wall.path.is_empty() && !w.requested {
            w.requested = true;
            *redecode = true;
        }
        return Out::default();
    };
    if cur.w != bw || cur.h != bh {
        // the screen changed size: draw what we have scaled, a new decode follows
        if let Some(img) = crate::gfx::Image::from_rgba(cur.w, cur.h, cur.rgba.clone()) {
            cv.image_rrect(&img, 0.0, 0.0, cv.w(), cv.h(), 0.0);
        }
        w.drawn = true;
        if !w.requested {
            w.requested = true;
            *redecode = true;
        }
        return Out::default();
    }
    let t = w
        .since
        .map(|s| crate::anim::progress(s, ctx.now, FADE_MS))
        .unwrap_or(1.0);
    let eased = crate::anim::std_ease(t);
    let data = cv.pm.data_mut();
    match &w.prev {
        Some(prev) if t < 1.0 && prev.w == bw && prev.h == bh => {
            let n = prev.rgba.len().min(data.len());
            data[..n].copy_from_slice(&prev.rgba[..n]);
            blit(data, &cur, (eased * 255.0) as u32);
        }
        _ if t < 1.0 && w.had => {
            // the old picture is not kept in memory: the new one lights up from a dim start
            let k = (0.35 + 0.65 * eased) * 255.0;
            let k = k as u32;
            for (d, s) in data.iter_mut().zip(cur.rgba.iter()) {
                *d = ((*s as u32 * k + 127) / 255) as u8;
            }
            for a in data.chunks_exact_mut(4) {
                a[3] = 255;
            }
        }
        _ => blit(data, &cur, 255),
    }
    let anim = t < 1.0 && (w.prev.is_some() || w.had);
    if !anim {
        // done: let the pixels go (the buffer has them now)
        w.prev = None;
        w.cur = None;
        w.drawn = true;
        w.had = true;
        w.requested = false;
    }
    Out {
        anim,
        input: None,
        done: false,
        damage: None,
    }
}
