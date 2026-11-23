//! Drawing: a thin layer over tiny-skia for the shapes the shell is made of (pills, cards, Material 3 shapes,
//! wavy lines), plus the glyph and icon blitter. Everything takes logical pixels; the canvas scales.

use std::collections::HashMap;
use std::f32::consts::{PI, TAU};

use tiny_skia::{
    FillRule, LineCap, LinearGradient, Mask, Paint, Path, PathBuilder, Pattern, Pixmap, PixmapMut,
    Point, SpreadMode, Stroke, Transform,
};

use crate::color::Rgba;
use crate::icons_data::ICONS;
use crate::text::{Face, Fonts};

pub struct IconCache {
    masks: HashMap<(u16, u16), Box<[u8]>>,
}

impl IconCache {
    fn new() -> Self {
        IconCache {
            masks: HashMap::new(),
        }
    }

    fn find(name: &str) -> Option<usize> {
        ICONS.binary_search_by(|(n, _)| (*n).cmp(name)).ok()
    }

    fn mask(&mut self, idx: usize, px: u16) -> Option<&[u8]> {
        let key = (idx as u16, px);
        if !self.masks.contains_key(&key) {
            if self.masks.len() > 400 {
                self.masks.clear();
            }
            let m = raster_icon(ICONS[idx].1, px)?;
            self.masks.insert(key, m);
        }
        self.masks.get(&key).map(|b| &b[..])
    }
}

fn kurbo_to_skia(bez: &kurbo::BezPath) -> Option<Path> {
    use kurbo::PathEl::*;
    let mut pb = PathBuilder::new();
    for el in bez.elements() {
        match *el {
            MoveTo(p) => pb.move_to(p.x as f32, p.y as f32),
            LineTo(p) => pb.line_to(p.x as f32, p.y as f32),
            QuadTo(a, p) => pb.quad_to(a.x as f32, a.y as f32, p.x as f32, p.y as f32),
            CurveTo(a, b, p) => pb.cubic_to(
                a.x as f32, a.y as f32, b.x as f32, b.y as f32, p.x as f32, p.y as f32,
            ),
            ClosePath => pb.close(),
        }
    }
    pb.finish()
}

fn raster_icon(d: &str, px: u16) -> Option<Box<[u8]>> {
    let bez = kurbo::BezPath::from_svg(d).ok()?;
    let path = kurbo_to_skia(&bez)?;
    let mut m = Mask::new(px as u32, px as u32)?;
    // most symbols use viewBox "0 -960 960 960"; a few older ones are drawn on a 24-unit grid
    let small = kurbo::Shape::bounding_box(&bez).y0 >= -0.5;
    let (s, dy) = if small {
        (px as f32 / 24.0, 0.0)
    } else {
        (px as f32 / 960.0, px as f32)
    };
    m.fill_path(
        &path,
        FillRule::Winding,
        true,
        Transform::from_row(s, 0.0, 0.0, s, 0.0, dy),
    );
    Some(m.data().to_vec().into_boxed_slice())
}

/// The still part of an animated surface, kept between frames: (key, pixels, hit areas it made).
pub struct Layer {
    pub key: u64,
    pub pm: Pixmap,
    pub hits: Vec<crate::ui::Hit>,
    pub used: std::time::Instant,
}

/// Shared drawing resources: fonts and icon masks.
pub struct Gfx {
    pub fonts: Fonts,
    pub icons: IconCache,
    pub layers: HashMap<u64, Layer>,
    /// pictures scaled into a shape, ready to turn: (picture, size, shape) -> (pixels, mask)
    pub covers: HashMap<(usize, u32, u64), (Pixmap, Vec<u8>)>,
    /// a cached still layer was redrawn during this frame (so the whole surface changed)
    pub rebuilt: bool,
}

impl Gfx {
    pub fn new() -> Self {
        Gfx {
            fonts: Fonts::new(),
            icons: IconCache::new(),
            layers: HashMap::new(),
            covers: HashMap::new(),
            rebuilt: false,
        }
    }
    /// Drop still layers nobody used for a while.
    pub fn trim_layers(&mut self) {
        let now = std::time::Instant::now();
        self.layers
            .retain(|_, l| now.duration_since(l.used) < std::time::Duration::from_secs(2));
        if self.covers.len() > 4 {
            self.covers.clear();
        }
        if self.layers.is_empty() {
            self.layers.shrink_to_fit();
        }
    }
    pub fn trim(&mut self) {
        self.fonts.trim();
        self.icons.masks.clear();
        self.icons.masks.shrink_to_fit();
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Material 3 expressive shapes, made from polar curves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Circle,
    Cookie(u8),
    Sunny,
    VerySunny,
    Flower,
    Clover,
    Pentagon,
    Hexagon,
    Square,
}

impl Shape {
    pub fn parse(s: &str) -> Shape {
        match s {
            "circle" => Shape::Circle,
            "cookie" | "cookie9" => Shape::Cookie(9),
            "cookie4" => Shape::Cookie(4),
            "cookie6" => Shape::Cookie(6),
            "cookie7" => Shape::Cookie(7),
            "cookie12" => Shape::Cookie(12),
            "sunny" => Shape::Sunny,
            "flower" => Shape::Flower,
            "clover" | "clover4" => Shape::Clover,
            "pentagon" => Shape::Pentagon,
            "hexagon" => Shape::Hexagon,
            "square" => Shape::Square,
            "sun" => Shape::Sunny,
            _ => Shape::VerySunny,
        }
    }

    /// Radius at angle `t` for a shape of unit radius.
    fn r(self, t: f32) -> f32 {
        match self {
            Shape::Circle => 1.0,
            Shape::Cookie(n) => 0.93 + 0.07 * (n as f32 * t).cos(),
            Shape::Sunny => 0.95 + 0.05 * (8.0 * t).cos(),
            Shape::VerySunny => 0.9 + 0.1 * (8.0 * t).cos(),
            Shape::Flower => {
                let c = (8.0 * t).cos();
                0.84 + 0.16 * c.signum() * c.abs().powf(0.7)
            }
            Shape::Clover => {
                let c = (4.0 * t).cos().abs();
                0.62 + 0.38 * c.powf(0.55)
            }
            Shape::Pentagon => rounded_polygon(5, t, 0.16),
            Shape::Hexagon => rounded_polygon(6, t, 0.14),
            Shape::Square => rounded_polygon(4, t + PI / 4.0, 0.34),
        }
    }
}

fn rounded_polygon(n: u32, t: f32, round: f32) -> f32 {
    let seg = TAU / n as f32;
    let a = (t + PI / 2.0).rem_euclid(seg) - seg / 2.0;
    let apothem = (PI / n as f32).cos();
    let sharp = apothem / a.cos();
    // blend toward a circle near the corners so they come out soft
    let circ = apothem + (1.0 - apothem) * 0.55;
    let k = (a.abs() / (seg / 2.0)).powf(3.0) * round * 2.5;
    (sharp * (1.0 - k.min(1.0)) + circ * k.min(1.0)).min(1.0)
}

pub fn shape_path(shape: Shape, cx: f32, cy: f32, r: f32, rot: f32) -> Option<Path> {
    if shape == Shape::Circle {
        return PathBuilder::from_circle(cx, cy, r);
    }
    let n = 120;
    let mut pb = PathBuilder::new();
    for i in 0..=n {
        let t = i as f32 / n as f32 * TAU;
        let rr = r * shape.r(t - rot);
        let (x, y) = (cx + rr * t.cos(), cy + rr * t.sin());
        if i == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    pb.close();
    pb.finish()
}

pub fn rrect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let mut pb = PathBuilder::new();
    if r <= 0.01 {
        pb.push_rect(tiny_skia::Rect::from_xywh(x, y, w, h)?);
        return pb.finish();
    }
    let k = 0.5523 * r;
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

/// A decoded image in tiny-skia's premultiplied RGBA.
pub struct Image {
    pub pm: Pixmap,
}

impl Image {
    pub fn from_rgba(w: u32, h: u32, mut data: Vec<u8>) -> Option<Image> {
        for px in data.chunks_exact_mut(4) {
            let a = px[3] as u32;
            if a < 255 {
                px[0] = ((px[0] as u32 * a + 127) / 255) as u8;
                px[1] = ((px[1] as u32 * a + 127) / 255) as u8;
                px[2] = ((px[2] as u32 * a + 127) / 255) as u8;
            }
        }
        let pm = Pixmap::from_vec(data, tiny_skia::IntSize::from_wh(w, h)?)?;
        Some(Image { pm })
    }
    pub fn w(&self) -> f32 {
        self.pm.width() as f32
    }
    pub fn h(&self) -> f32 {
        self.pm.height() as f32
    }
}

pub struct Canvas<'a> {
    pub pm: PixmapMut<'a>,
    pub s: f32,
    pub g: &'a mut Gfx,
    /// global opacity (fades)
    pub alpha: f32,
    /// translation in logical pixels
    pub ox: f32,
    pub oy: f32,
}

fn paint(c: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c.sk());
    p.anti_alias = true;
    p
}

impl<'a> Canvas<'a> {
    pub fn new(pm: PixmapMut<'a>, s: f32, g: &'a mut Gfx) -> Canvas<'a> {
        Canvas {
            pm,
            s,
            g,
            alpha: 1.0,
            ox: 0.0,
            oy: 0.0,
        }
    }

    /// Draw the still part of a surface through a cache: `f` only runs when `key` changed.
    /// The hit areas `f` adds are kept with the pixels and added again on every frame.
    pub fn cached(
        &mut self,
        slot: u64,
        key: u64,
        hits: &mut crate::ui::Hits,
        f: impl FnOnce(&mut Canvas, &mut crate::ui::Hits),
    ) {
        let (w, h) = (self.pm.width(), self.pm.height());
        let fresh = match self.g.layers.get(&slot) {
            Some(l) => l.key != key || l.pm.width() != w || l.pm.height() != h,
            None => true,
        };
        if fresh {
            let Some(mut pm) = Pixmap::new(w, h) else {
                return;
            };
            let mut lh = crate::ui::Hits::new(hits.hover, hits.pressed);
            lh.ox = hits.ox;
            lh.oy = hits.oy;
            lh.base = hits.list.len() as u32;
            {
                let mut cv = Canvas::new(pm.as_mut(), self.s, &mut *self.g);
                cv.alpha = self.alpha;
                cv.ox = self.ox;
                cv.oy = self.oy;
                f(&mut cv, &mut lh);
            }
            self.g.layers.insert(
                slot,
                Layer {
                    key,
                    pm,
                    hits: lh.list,
                    used: std::time::Instant::now(),
                },
            );
            self.g.rebuilt = true;
        }
        if let Some(l) = self.g.layers.get_mut(&slot) {
            l.used = std::time::Instant::now();
            let src = l.pm.data();
            let dst = self.pm.data_mut();
            let n = src.len().min(dst.len());
            dst[..n].copy_from_slice(&src[..n]);
            hits.list.extend(l.hits.iter().cloned());
        }
    }

    pub fn w(&self) -> f32 {
        self.pm.width() as f32 / self.s
    }
    pub fn h(&self) -> f32 {
        self.pm.height() as f32 / self.s
    }

    fn tf(&self) -> Transform {
        Transform::from_row(self.s, 0.0, 0.0, self.s, self.ox * self.s, self.oy * self.s)
    }

    fn col(&self, c: Rgba) -> Rgba {
        c.mul_alpha(self.alpha)
    }

    pub fn clear(&mut self) {
        self.pm.fill(tiny_skia::Color::TRANSPARENT);
    }

    pub fn fill_path(&mut self, p: &Path, c: Rgba) {
        let c = self.col(c);
        if c.a <= 0.002 {
            return;
        }
        let t = self.tf();
        self.pm.fill_path(p, &paint(c), FillRule::Winding, t, None);
    }

    pub fn stroke_path(&mut self, p: &Path, width: f32, c: Rgba) {
        let c = self.col(c);
        if c.a <= 0.002 {
            return;
        }
        let t = self.tf();
        let st = Stroke {
            width,
            line_cap: LineCap::Round,
            ..Default::default()
        };
        self.pm.stroke_path(p, &paint(c), &st, t, None);
    }

    fn target(&mut self) -> crate::raster::Target<'_> {
        let (w, h) = (self.pm.width() as i32, self.pm.height() as i32);
        crate::raster::Target {
            data: self.pm.data_mut(),
            w,
            h,
        }
    }

    fn dev(&self, x: f32, y: f32) -> (f32, f32) {
        ((x + self.ox) * self.s, (y + self.oy) * self.s)
    }

    pub fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Rgba) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let c8 = self.col(c).premul8();
        let (dx, dy) = self.dev(x, y);
        let s = self.s;
        self.target().rrect(dx, dy, w * s, h * s, r * s, c8);
    }

    pub fn rrect_stroke(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, width: f32, c: Rgba) {
        if let Some(p) = rrect_path(
            x + width / 2.0,
            y + width / 2.0,
            w - width,
            h - width,
            r - width / 2.0,
        ) {
            self.stroke_path(&p, width, c);
        }
    }

    /// Vertical gradient inside a rounded rect (the glass shine).
    pub fn rrect_vgrad(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        top: Rgba,
        bottom: Rgba,
        mid: f32,
    ) {
        let Some(p) = rrect_path(x, y, w, h, r) else {
            return;
        };
        let stops = vec![
            tiny_skia::GradientStop::new(0.0, self.col(top).sk()),
            tiny_skia::GradientStop::new(mid.clamp(0.01, 1.0), self.col(bottom).sk()),
        ];
        let Some(sh) = LinearGradient::new(
            Point::from_xy(0.0, y),
            Point::from_xy(0.0, y + h),
            stops,
            SpreadMode::Pad,
            Transform::identity(),
        ) else {
            return;
        };
        let p2 = Paint {
            shader: sh,
            anti_alias: true,
            ..Default::default()
        };
        let t = self.tf();
        self.pm.fill_path(&p, &p2, FillRule::Winding, t, None);
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        if let Some(p) = PathBuilder::from_circle(cx, cy, r.max(0.1)) {
            self.fill_path(&p, c);
        }
    }

    pub fn ring(&mut self, cx: f32, cy: f32, r: f32, width: f32, c: Rgba) {
        self.wavy_arc(cx, cy, r, 0.0, 0.0, 0.0, 0.0, TAU, width, c);
    }

    pub fn shape(&mut self, shape: Shape, cx: f32, cy: f32, r: f32, rot: f32, c: Rgba) {
        if let Some(p) = shape_path(shape, cx, cy, r, rot) {
            self.fill_path(&p, c);
        }
    }

    /// An arc from `start` sweeping `sweep` radians (clockwise, 0 = 3 o'clock).
    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, start: f32, sweep: f32, width: f32, c: Rgba) {
        self.wavy_arc(cx, cy, r, 0.0, 0.0, 0.0, start, sweep, width, c);
    }

    /// A ring that wobbles `n` times around (`amp` px), drawn from `start` over `sweep`.
    pub fn wavy_arc(
        &mut self,
        cx: f32,
        cy: f32,
        r: f32,
        amp: f32,
        n: f32,
        phase: f32,
        start: f32,
        sweep: f32,
        width: f32,
        c: Rgba,
    ) {
        let c8 = self.col(c).premul8();
        let (dx, dy) = self.dev(cx, cy);
        let s = self.s;
        self.target().polar_stroke(
            dx,
            dy,
            r * s,
            amp * s,
            n,
            phase,
            start,
            sweep,
            width * s,
            c8,
        );
    }

    /// A horizontal wave from x0 to x1 (Material 3 wavy progress).
    pub fn wavy_line(
        &mut self,
        x0: f32,
        x1: f32,
        y: f32,
        amp: f32,
        wavelength: f32,
        phase: f32,
        width: f32,
        c: Rgba,
    ) {
        if x1 - x0 < 0.5 {
            return;
        }
        let c8 = self.col(c).premul8();
        let (a, yy) = self.dev(x0, y);
        let (b, _) = self.dev(x1, y);
        let s = self.s;
        self.target()
            .wave_h(a, b, yy, amp * s, wavelength * s, phase, width * s, c8);
    }

    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, c: Rgba) {
        if (y0 - y1).abs() < 0.01 {
            self.wavy_line(x0.min(x1), x0.max(x1), y0, 0.0, 10.0, 0.0, width, c);
            return;
        }
        let mut pb = PathBuilder::new();
        pb.move_to(x0, y0);
        pb.line_to(x1, y1);
        if let Some(p) = pb.finish() {
            self.stroke_path(&p, width, c);
        }
    }

    /// A soft shadow under a rounded rect: a few widening, fading outlines (cheap, no blur pass).
    pub fn shadow(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, size: f32, strength: f32) {
        let steps = 6;
        for i in 0..steps {
            let k = (i as f32 + 1.0) / steps as f32;
            let grow = size * k;
            let a = strength * 0.16 * (1.0 - k).powf(1.6);
            self.rrect(
                x - grow * 0.6,
                y - grow * 0.35,
                w + grow * 1.2,
                h + grow * 1.2,
                r + grow,
                crate::color::rgba(0.0, 0.0, 0.0, a),
            );
        }
    }

    /// An image filling a path (cover art in a shape, avatars), scaled to cover the box.
    pub fn image_in(&mut self, img: &Image, clip: &Path, x: f32, y: f32, w: f32, h: f32) {
        let sc = (w / img.w()).max(h / img.h());
        let dx = x + (w - img.w() * sc) / 2.0;
        let dy = y + (h - img.h() * sc) / 2.0;
        let it = Transform::from_row(sc, 0.0, 0.0, sc, dx, dy);
        let pat = Pattern::new(
            img.pm.as_ref(),
            SpreadMode::Pad,
            tiny_skia::FilterQuality::Bilinear,
            self.alpha,
            it,
        );
        let p = Paint {
            shader: pat,
            anti_alias: true,
            ..Default::default()
        };
        let t = self.tf();
        self.pm.fill_path(clip, &p, FillRule::Winding, t, None);
    }

    pub fn image_rrect(&mut self, img: &Image, x: f32, y: f32, w: f32, h: f32, r: f32) {
        if let Some(p) = rrect_path(x, y, w, h, r) {
            self.image_in(img, &p, x, y, w, h);
        }
    }

    /// A picture in a shape, turned by `rot`. The scaled picture and the shape's mask are kept, so a turning
    /// cover costs one bilinear pass over its own pixels per frame.
    pub fn image_shape(&mut self, img: &Image, shape: Shape, cx: f32, cy: f32, r: f32, rot: f32) {
        let size = (r * 2.0 * self.s).round().max(2.0) as u32;
        let key = (img as *const Image as usize, size, {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            format!("{shape:?}").hash(&mut h);
            h.finish()
        });
        if !self.g.covers.contains_key(&key) {
            let Some(mut pm) = Pixmap::new(size, size) else {
                return;
            };
            let sc = (size as f32 / img.w()).max(size as f32 / img.h());
            let dx = (size as f32 - img.w() * sc) / 2.0;
            let dy = (size as f32 - img.h() * sc) / 2.0;
            pm.draw_pixmap(
                0,
                0,
                img.pm.as_ref(),
                &tiny_skia::PixmapPaint {
                    quality: tiny_skia::FilterQuality::Bilinear,
                    ..Default::default()
                },
                Transform::from_row(sc, 0.0, 0.0, sc, dx, dy),
                None,
            );
            let half = size as f32 / 2.0;
            let mut m = Mask::new(size, size).unwrap();
            if let Some(p) = shape_path(shape, half, half, half, 0.0) {
                m.fill_path(&p, FillRule::Winding, true, Transform::identity());
            }
            if self.g.covers.len() > 8 {
                self.g.covers.clear();
            }
            self.g.covers.insert(key, (pm, m.data().to_vec()));
        }
        let (dx, dy) = self.dev(cx, cy);
        let x0 = (dx - size as f32 / 2.0).round() as i32;
        let y0 = (dy - size as f32 / 2.0).round() as i32;
        let alpha = (self.alpha.clamp(0.0, 1.0) * 256.0) as u32;
        let (w, h) = (self.pm.width() as i32, self.pm.height() as i32);
        let Some((pm, mask)) = self.g.covers.get(&key) else {
            return;
        };
        let mut t = crate::raster::Target {
            data: self.pm.data_mut(),
            w,
            h,
        };
        t.rotated(pm, mask, size as i32, x0, y0, rot, alpha);
    }

    /// Blend an 8-bit coverage mask at device pixel (x0, y0).
    fn blit(&mut self, x0: i32, y0: i32, w: u32, h: u32, cov: &[u8], c: [u32; 4]) {
        if c[3] == 0 {
            return;
        }
        let pw = self.pm.width() as i32;
        let ph = self.pm.height() as i32;
        let data = self.pm.data_mut();
        let ys = 0.max(-y0);
        let ye = (h as i32).min(ph - y0);
        let xs = 0.max(-x0);
        let xe = (w as i32).min(pw - x0);
        if ys >= ye || xs >= xe {
            return;
        }
        for yy in ys..ye {
            let row = ((y0 + yy) * pw) as usize * 4;
            let crow = (yy * w as i32) as usize;
            for xx in xs..xe {
                let m = cov[crow + xx as usize] as u32;
                if m == 0 {
                    continue;
                }
                let i = row + ((x0 + xx) as usize) * 4;
                let sa = (c[3] * m + 127) / 255;
                let inv = 255 - sa;
                let d = &mut data[i..i + 4];
                d[0] = ((c[0] * m + 127) / 255 + (d[0] as u32 * inv + 127) / 255).min(255) as u8;
                d[1] = ((c[1] * m + 127) / 255 + (d[1] as u32 * inv + 127) / 255).min(255) as u8;
                d[2] = ((c[2] * m + 127) / 255 + (d[2] as u32 * inv + 127) / 255).min(255) as u8;
                d[3] = (sa + (d[3] as u32 * inv + 127) / 255).min(255) as u8;
            }
        }
    }

    /// Draw text with its baseline at `y`. Returns the width in logical px.
    pub fn text_base(
        &mut self,
        x: f32,
        y: f32,
        s: &str,
        face: Face,
        size: f32,
        c: Rgba,
        align: Align,
    ) -> f32 {
        let c8 = self.col(c).premul8();
        let sc = self.s;
        let px = size * sc;
        let wdev = if align == Align::Left {
            0.0
        } else {
            self.g.fonts.width(s, face, px)
        };
        let start = match align {
            Align::Left => (x + self.ox) * sc,
            Align::Center => (x + self.ox) * sc - wdev / 2.0,
            Align::Right => (x + self.ox) * sc - wdev,
        };
        let by = ((y + self.oy) * sc).round() as i32;
        let mut glyphs: Vec<(i32, i32, u32, u32, *const u8)> = Vec::new();
        let w = self.g.fonts.layout(s, face, px, start.round(), |gx, g| {
            glyphs.push((gx as i32 + g.left, by + g.top, g.w, g.h, g.cov.as_ptr()));
        });
        for (gx, gy, gw, gh, p) in glyphs {
            // SAFETY: the glyph cache is not touched between layout and this loop.
            let cov = unsafe { std::slice::from_raw_parts(p, (gw * gh) as usize) };
            self.blit(gx, gy, gw, gh, cov, c8);
        }
        w / sc
    }

    /// Draw text vertically centred on `cy` (by cap height, so digits and capitals sit in the middle).
    pub fn text(
        &mut self,
        x: f32,
        cy: f32,
        s: &str,
        face: Face,
        size: f32,
        c: Rgba,
        align: Align,
    ) -> f32 {
        let ch = self.g.fonts.cap_height(face, size * self.s) / self.s;
        self.text_base(x, cy + ch / 2.0, s, face, size, c, align)
    }

    /// Text cut to `max` px with an ellipsis.
    pub fn text_max(
        &mut self,
        x: f32,
        cy: f32,
        s: &str,
        face: Face,
        size: f32,
        c: Rgba,
        align: Align,
        max: f32,
    ) -> f32 {
        let t = self.g.fonts.elide(s, face, size * self.s, max * self.s);
        self.text(x, cy, &t, face, size, c, align)
    }

    pub fn measure(&mut self, s: &str, face: Face, size: f32) -> f32 {
        self.g.fonts.width(s, face, size * self.s) / self.s
    }

    /// A Material Symbols icon centred on (cx, cy), `size` px tall.
    pub fn icon(&mut self, name: &str, cx: f32, cy: f32, size: f32, c: Rgba) {
        let Some(idx) = IconCache::find(name).or_else(|| IconCache::find("apps")) else {
            return;
        };
        let px = (size * self.s).round().max(4.0) as u16;
        let c8 = self.col(c).premul8();
        let x0 = ((cx + self.ox) * self.s - px as f32 / 2.0).round() as i32;
        let y0 = ((cy + self.oy) * self.s - px as f32 / 2.0).round() as i32;
        let Some(m) = self.g.icons.mask(idx, px) else {
            return;
        };
        let m: *const u8 = m.as_ptr();
        // SAFETY: the mask lives in the cache, which is not modified while blitting.
        let cov = unsafe { std::slice::from_raw_parts(m, px as usize * px as usize) };
        self.blit(x0, y0, px as u32, px as u32, cov, c8);
    }
}
