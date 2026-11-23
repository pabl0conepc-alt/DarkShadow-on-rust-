//! The wallpaper picker (SUPER + W, or right-click the desktop): your wallpapers in a row; Enter sets one,
//! R picks one at random. The colors of the whole shell follow the picture.

use crate::color::rgba;
use crate::gfx::{Align, Canvas, Shape};
use crate::shell::SHADOW;
use crate::text::Face;
use crate::ui::{Act, Ctx, Hits, Out, Ui, icon_button};

pub fn draw(cv: &mut Canvas, ctx: &Ctx, ui: &mut Ui, hits: &mut Hits) -> Out {
    let now = ctx.now;
    let pk = &ui.picker;
    if pk.rev.gone(now) {
        return Out {
            anim: false,
            input: Some(vec![]),
            done: true,
            damage: None,
        };
    }
    let p = ctx.th.pal;
    let k = pk.rev.value(now);
    let m = SHADOW as f32;
    let top = ctx.cfg.bar.position == "top";
    let (w, h) = (cv.w() - 2.0 * m, cv.h() - 2.0 * m);
    let off = (1.0 - k) * 30.0;
    let (x, y) = (m, m + if top { -off } else { off });
    cv.alpha = k;
    if ctx.cfg.theme.shadows {
        cv.shadow(x, y + 4.0, w, h, 30.0, 18.0, 0.7);
    }
    cv.rrect(x, y, w, h, 30.0, p.surface);
    cv.rrect_stroke(x, y, w, h, 30.0, 1.0, p.line);
    cv.icon("wallpaper", x + 34.0, y + 34.0, 22.0, p.primary);
    cv.text(
        x + 56.0,
        y + 34.0,
        "Wallpapers",
        Face::Semi,
        16.0,
        p.fg,
        Align::Left,
    );
    let dir = ctx.cfg.wallpaper.dir.clone();
    cv.text_max(
        x + 160.0,
        y + 34.0,
        &dir,
        Face::Regular,
        12.0,
        p.dim,
        Align::Left,
        w - 320.0,
    );
    icon_button(
        cv,
        hits,
        x + w - 76.0,
        y + 34.0,
        18.0,
        "shuffle",
        p.fg,
        p.card,
        p.card_hi,
        Act::WallShuffle,
    );
    icon_button(
        cv,
        hits,
        x + w - 34.0,
        y + 34.0,
        18.0,
        "close",
        p.fg,
        p.card,
        p.card_hi,
        Act::CloseAll,
    );

    let n = pk.list.len();
    if n == 0 {
        cv.shape(
            Shape::Cookie(9),
            x + w / 2.0,
            y + h / 2.0 + 10.0,
            36.0,
            0.0,
            p.container,
        );
        cv.icon(
            "image",
            x + w / 2.0,
            y + h / 2.0 + 10.0,
            30.0,
            p.ink_container,
        );
        cv.text(
            x + w / 2.0,
            y + h / 2.0 + 70.0,
            &format!("Put pictures in {} and they show up here", dir),
            Face::Medium,
            13.0,
            p.dim,
            Align::Center,
        );
        cv.alpha = 1.0;
        return Out {
            anim: pk.rev.animating(now),
            input: None,
            done: false,
            damage: None,
        };
    }
    let (tw, th, gap) = (172.0, 108.0, 12.0);
    let visible = ((w - 40.0 + gap) / (tw + gap)).floor().max(1.0) as usize;
    let first = pk
        .sel
        .saturating_sub(visible / 2)
        .min(n.saturating_sub(visible));
    let row_w = visible.min(n) as f32 * (tw + gap) - gap;
    let x0 = x + (w - row_w) / 2.0;
    let ty = y + 84.0;
    let mut want = Vec::new();
    for (j, i) in (first..n.min(first + visible)).enumerate() {
        let tx = x0 + j as f32 * (tw + gap);
        let sel = i == pk.sel;
        let hov = hits.add(tx, ty, tw, th, Act::Wallpaper(i));
        let lift = if sel { 0.0 } else { 6.0 };
        let (bx, by, bw, bh) = (tx + lift, ty + lift, tw - lift * 2.0, th - lift * 2.0);
        match pk.thumbs.get(&i) {
            Some(img) => cv.image_rrect(img, bx, by, bw, bh, 18.0),
            None => {
                cv.rrect(bx, by, bw, bh, 18.0, p.card);
                want.push(i);
            }
        }
        if sel {
            cv.rrect_stroke(tx - 4.0, ty - 4.0, tw + 8.0, th + 8.0, 22.0, 3.0, p.primary);
        } else if hov {
            cv.rrect(bx, by, bw, bh, 18.0, rgba(1.0, 1.0, 1.0, 0.08));
        }
        let name = pk.list[i]
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        cv.text_max(
            tx + tw / 2.0,
            ty + th + 20.0,
            &name,
            Face::Medium,
            12.0,
            if sel { p.fg } else { p.dim },
            Align::Center,
            tw,
        );
        if ctx.st.wall.path == pk.list[i].to_string_lossy() {
            cv.circle(tx + tw - 18.0, ty + 18.0, 11.0, p.primary);
            cv.icon("check", tx + tw - 18.0, ty + 18.0, 15.0, p.ink_primary);
        }
    }
    cv.text(
        x + w / 2.0,
        y + h - 22.0,
        "← → choose · Enter set · R random · Esc close",
        Face::Regular,
        11.0,
        p.dim,
        Align::Center,
    );
    cv.alpha = 1.0;
    // thumbnails to load (picked up by the app after this pass)
    ui.want_thumbs = want;
    Out {
        anim: pk.rev.animating(now),
        input: None,
        done: false,
        damage: None,
    }
}
