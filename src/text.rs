//! Text: Google Sans Flex (embedded, variable) rasterized with ab_glyph into a small coverage cache.
//!
//! No shaping engine: Latin, Cyrillic, Greek and the like render correctly, which is what a shell's labels need.
//! A system font (fontconfig's sans-serif) is memory-mapped on the first character the main font lacks.

use std::collections::HashMap;

use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont, VariableFont, point};

static MAIN: &[u8] = include_bytes!("../assets/fonts/GoogleSansFlex.ttf");

/// The faces the interface uses (instances of the variable font).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum Face {
    Regular = 0,
    Medium = 1,
    Semi = 2,
    Bold = 3,
    /// Tall and narrow: the big clock.
    Display = 4,
    /// Bold and a bit wide: big numbers on gauges and timers.
    Numbers = 5,
    Light = 6,
    /// the heaviest weight: the clock's digits
    Black = 7,
}

const FACES: [(Face, f32, f32); 8] = [
    (Face::Regular, 400.0, 100.0),
    (Face::Medium, 500.0, 100.0),
    (Face::Semi, 600.0, 100.0),
    (Face::Bold, 700.0, 100.0),
    (Face::Display, 560.0, 25.0),
    (Face::Numbers, 720.0, 104.0),
    (Face::Light, 300.0, 100.0),
    (Face::Black, 900.0, 100.0),
];

pub struct Glyph {
    pub left: i32,
    pub top: i32,
    pub w: u32,
    pub h: u32,
    pub cov: Box<[u8]>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    face: u8,
    fb: bool,
    id: u16,
    size: u16,
    sub: u8,
}

pub struct Fonts {
    faces: Vec<FontRef<'static>>,
    k: Vec<f32>,
    fallback: Option<FontRef<'static>>,
    fallback_k: f32,
    fallback_tried: bool,
    cache: HashMap<Key, Option<Glyph>>,
}

fn em_factor(f: &FontRef<'_>) -> f32 {
    f.height_unscaled() / f.units_per_em().unwrap_or(1000.0)
}

fn mmap_file(path: &str) -> Option<&'static [u8]> {
    use std::os::fd::AsRawFd;
    let file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len() as usize;
    if len == 0 {
        return None;
    }
    // SAFETY: a read-only private mapping of a font file that lives for the whole process.
    unsafe {
        let p = libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ, libc::MAP_PRIVATE, file.as_raw_fd(), 0);
        if p == libc::MAP_FAILED {
            return None;
        }
        Some(std::slice::from_raw_parts(p as *const u8, len))
    }
}

impl Fonts {
    pub fn new() -> Fonts {
        let base = FontRef::try_from_slice(MAIN).expect("embedded font");
        let mut faces = Vec::new();
        let mut k = Vec::new();
        for (_, wght, wdth) in FACES {
            let mut f = base.clone();
            f.set_variation(b"wght", wght);
            f.set_variation(b"wdth", wdth);
            f.set_variation(b"opsz", 18.0);
            k.push(em_factor(&f));
            faces.push(f);
        }
        Fonts { faces, k, fallback: None, fallback_k: 1.0, fallback_tried: false, cache: HashMap::new() }
    }

    fn load_fallback(&mut self) {
        self.fallback_tried = true;
        let out = std::process::Command::new("fc-match").args(["-f", "%{file}", "sans-serif"]).output();
        if let Ok(o) = out {
            let path = String::from_utf8_lossy(&o.stdout).to_string();
            if let Some(data) = mmap_file(path.trim())
                && let Ok(f) = FontRef::try_from_slice(data)
            {
                self.fallback_k = em_factor(&f);
                self.fallback = Some(f);
            }
        }
    }

    /// The glyph for `c`: (use fallback, id).
    fn lookup(&mut self, face: Face, c: char) -> (bool, GlyphId) {
        let id = self.faces[face as usize].glyph_id(c);
        if id.0 != 0 || c.is_control() {
            return (false, id);
        }
        if !self.fallback_tried {
            self.load_fallback();
        }
        if let Some(fb) = &self.fallback {
            let id2 = fb.glyph_id(c);
            if id2.0 != 0 {
                return (true, id2);
            }
        }
        (false, id)
    }

    fn scaled(&self, face: Face, fb: bool, size: f32) -> ab_glyph::PxScaleFont<&FontRef<'static>> {
        if fb && let Some(f) = &self.fallback {
            return f.as_scaled(PxScale::from(size * self.fallback_k));
        }
        let f = &self.faces[face as usize];
        f.as_scaled(PxScale::from(size * self.k[face as usize]))
    }



    /// Height of capital letters / digits (for centring numbers optically).
    pub fn cap_height(&self, face: Face, size: f32) -> f32 {
        let f = &self.faces[face as usize];
        let s = self.scaled(face, false, size);
        let id = f.glyph_id('H');
        let g = id.with_scale(s.scale());
        f.outline_glyph(g).map(|o| -o.px_bounds().min.y).unwrap_or(s.ascent() * 0.72)
    }

    pub fn width(&mut self, s: &str, face: Face, size: f32) -> f32 {
        let mut x = 0.0;
        let mut prev: Option<(bool, GlyphId)> = None;
        for c in s.chars() {
            let (fb, id) = self.lookup(face, c);
            let sf = self.scaled(face, fb, size);
            if let Some((pfb, pid)) = prev
                && pfb == fb
            {
                x += sf.kern(pid, id);
            }
            x += sf.h_advance(id);
            prev = Some((fb, id));
        }
        x
    }

    /// Cut `s` to fit `max` pixels, ending with an ellipsis.
    pub fn elide(&mut self, s: &str, face: Face, size: f32, max: f32) -> String {
        if self.width(s, face, size) <= max {
            return s.to_string();
        }
        let ell = self.width("…", face, size);
        let mut out = String::new();
        let mut w = 0.0;
        for c in s.chars() {
            let cw = self.width(c.encode_utf8(&mut [0; 4]), face, size);
            if w + cw + ell > max {
                break;
            }
            w += cw;
            out.push(c);
        }
        let t = out.trim_end().to_string();
        t + "…"
    }

    /// Lay out `s` with its baseline at y=0 starting at x=0 and call `f` for every glyph bitmap.
    pub fn layout(&mut self, s: &str, face: Face, size: f32, x0: f32, mut f: impl FnMut(f32, &Glyph)) -> f32 {
        let mut x = x0;
        let mut prev: Option<(bool, GlyphId)> = None;
        let size_q = (size * 4.0).round().clamp(1.0, 65535.0) as u16;
        for c in s.chars() {
            let (fb, id) = self.lookup(face, c);
            let (adv, kern) = {
                let sf = self.scaled(face, fb, size);
                let kern = match prev {
                    Some((pfb, pid)) if pfb == fb => sf.kern(pid, id),
                    _ => 0.0,
                };
                (sf.h_advance(id), kern)
            };
            x += kern;
            if !c.is_whitespace() {
                let fx = x.floor();
                let sub = ((x - fx) * 4.0).floor().clamp(0.0, 3.0) as u8;
                let key = Key { face: face as u8, fb, id: id.0, size: size_q, sub };
                if !self.cache.contains_key(&key) {
                    if self.cache.len() > 3000 {
                        self.cache.clear();
                    }
                    let g = self.raster(face, fb, id, size, sub as f32 / 4.0);
                    self.cache.insert(key, g);
                }
                if let Some(Some(g)) = self.cache.get(&key) {
                    f(fx, g);
                }
            }
            x += adv;
            prev = Some((fb, id));
        }
        x - x0
    }

    fn raster(&self, face: Face, fb: bool, id: GlyphId, size: f32, sub: f32) -> Option<Glyph> {
        let (font, k) = if fb {
            (self.fallback.as_ref()?, self.fallback_k)
        } else {
            (&self.faces[face as usize], self.k[face as usize])
        };
        let g = id.with_scale_and_position(PxScale::from(size * k), point(sub, 0.0));
        let og = font.outline_glyph(g)?;
        let b = og.px_bounds();
        let w = (b.max.x - b.min.x).ceil().max(0.0) as u32;
        let h = (b.max.y - b.min.y).ceil().max(0.0) as u32;
        if w == 0 || h == 0 {
            return None;
        }
        let mut cov = vec![0u8; (w * h) as usize].into_boxed_slice();
        og.draw(|x, y, c| {
            if x < w && y < h {
                cov[(y * w + x) as usize] = (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            }
        });
        Some(Glyph { left: b.min.x as i32, top: b.min.y as i32, w, h, cov })
    }

    pub fn trim(&mut self) {
        self.cache.clear();
        self.cache.shrink_to_fit();
    }
}
