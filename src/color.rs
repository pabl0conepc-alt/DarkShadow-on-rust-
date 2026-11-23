//! Colors: straight-alpha RGBA in 0..1, with the HSL helpers the original QML palettes are written in.

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Rgba {
    Rgba { r, g, b, a }
}

pub const TRANSPARENT: Rgba = rgba(0.0, 0.0, 0.0, 0.0);
pub const WHITE: Rgba = rgba(1.0, 1.0, 1.0, 1.0);

impl Rgba {
    pub fn hex(s: &str) -> Option<Rgba> {
        let s = s.trim().trim_start_matches('#');
        let v = u32::from_str_radix(s, 16).ok()?;
        match s.len() {
            6 => Some(rgba(
                ((v >> 16) & 255) as f32 / 255.0,
                ((v >> 8) & 255) as f32 / 255.0,
                (v & 255) as f32 / 255.0,
                1.0,
            )),
            8 => Some(rgba(
                ((v >> 24) & 255) as f32 / 255.0,
                ((v >> 16) & 255) as f32 / 255.0,
                ((v >> 8) & 255) as f32 / 255.0,
                (v & 255) as f32 / 255.0,
            )),
            _ => None,
        }
    }

    /// Qt.hsla: hue, saturation and lightness all in 0..1.
    pub fn hsla(h: f32, s: f32, l: f32, a: f32) -> Rgba {
        let h = h.rem_euclid(1.0);
        let s = s.clamp(0.0, 1.0);
        let l = l.clamp(0.0, 1.0);
        if s <= 0.0 {
            return rgba(l, l, l, a);
        }
        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            l + s - l * s
        };
        let p = 2.0 * l - q;
        let f = |t: f32| {
            let t = t.rem_euclid(1.0);
            if t < 1.0 / 6.0 {
                p + (q - p) * 6.0 * t
            } else if t < 0.5 {
                q
            } else if t < 2.0 / 3.0 {
                p + (q - p) * (2.0 / 3.0 - t) * 6.0
            } else {
                p
            }
        };
        rgba(f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0), a)
    }

    /// (hue, saturation, lightness); hue is -1 for greys like Qt's hslHue.
    pub fn hsl(self) -> (f32, f32, f32) {
        let max = self.r.max(self.g).max(self.b);
        let min = self.r.min(self.g).min(self.b);
        let l = (max + min) / 2.0;
        let d = max - min;
        if d < 1e-4 {
            return (-1.0, 0.0, l);
        }
        let s = if l > 0.5 {
            d / (2.0 - max - min)
        } else {
            d / (max + min)
        };
        let h = if max == self.r {
            (self.g - self.b) / d + if self.g < self.b { 6.0 } else { 0.0 }
        } else if max == self.g {
            (self.b - self.r) / d + 2.0
        } else {
            (self.r - self.g) / d + 4.0
        };
        (h / 6.0, s, l)
    }

    pub fn alpha(self, a: f32) -> Rgba {
        Rgba { a, ..self }
    }

    pub fn mul_alpha(self, k: f32) -> Rgba {
        Rgba {
            a: self.a * k,
            ..self
        }
    }

    /// Qt.lighter(c, f) works in HSV value; this is close enough for the palettes.
    pub fn lighter(self, f: f32) -> Rgba {
        rgba(
            (self.r * f).min(1.0),
            (self.g * f).min(1.0),
            (self.b * f).min(1.0),
            self.a,
        )
    }

    pub fn mix(self, o: Rgba, t: f32) -> Rgba {
        let t = t.clamp(0.0, 1.0);
        rgba(
            self.r + (o.r - self.r) * t,
            self.g + (o.g - self.g) * t,
            self.b + (o.b - self.b) * t,
            self.a + (o.a - self.a) * t,
        )
    }

    pub fn luma(self) -> f32 {
        0.2126 * self.r + 0.7152 * self.g + 0.0722 * self.b
    }

    pub fn sk(self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba(
            self.r.clamp(0.0, 1.0),
            self.g.clamp(0.0, 1.0),
            self.b.clamp(0.0, 1.0),
            self.a.clamp(0.0, 1.0),
        )
        .unwrap_or(tiny_skia::Color::TRANSPARENT)
    }

    /// Premultiplied 0..255, for the coverage blitter.
    pub fn premul8(self) -> [u32; 4] {
        let a = self.a.clamp(0.0, 1.0);
        [
            (self.r.clamp(0.0, 1.0) * a * 255.0 + 0.5) as u32,
            (self.g.clamp(0.0, 1.0) * a * 255.0 + 0.5) as u32,
            (self.b.clamp(0.0, 1.0) * a * 255.0 + 0.5) as u32,
            (a * 255.0 + 0.5) as u32,
        ]
    }
}
