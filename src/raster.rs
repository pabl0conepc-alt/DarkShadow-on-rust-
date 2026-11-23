//! Fast software rasterizers for the shapes the shell draws most: rounded rectangles, rings and arcs (also
//! wavy ones), horizontal waves, and a picture turning inside a shape. They write premultiplied RGBA
//! directly and touch only the pixels the shape covers, which is several times quicker than a general path
//! fill for these cases.

use std::f32::consts::TAU;

/// `px * f / 255` for the four bytes of a pixel at once (two at a time in 16-bit lanes).
#[inline(always)]
fn scale(px: u32, f: u32) -> u32 {
    let mut rb = (px & 0x00ff_00ff) * f;
    rb = ((rb + 0x0080_0080 + ((rb >> 8) & 0x00ff_00ff)) >> 8) & 0x00ff_00ff;
    let mut ag = ((px >> 8) & 0x00ff_00ff) * f;
    ag = (ag + 0x0080_0080 + ((ag >> 8) & 0x00ff_00ff)) & 0xff00_ff00;
    rb | ag
}

#[inline(always)]
fn pack(c: [u32; 4]) -> u32 {
    c[0].min(255) | (c[1].min(255) << 8) | (c[2].min(255) << 16) | (c[3].min(255) << 24)
}

/// Source-over of a premultiplied packed pixel.
#[inline(always)]
fn over(d: u32, s: u32) -> u32 {
    let a = s >> 24;
    if a == 255 {
        return s;
    }
    s.wrapping_add(scale(d, 255 - a))
}

/// Blend `c` (premultiplied, 0..255) at coverage `k` (0..=256) over a pixel.
#[inline(always)]
fn blend(d: &mut [u8], c: [u32; 4], k: u32) {
    let s = if k >= 256 {
        pack(c)
    } else {
        scale(pack(c), k.min(255))
    };
    if s >> 24 == 0 {
        return;
    }
    let dv = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
    d.copy_from_slice(&over(dv, s).to_le_bytes());
}

/// Full-coverage run (the inside of a shape).
#[inline(always)]
fn blend_run(row: &mut [u8], c: [u32; 4]) {
    let s = pack(c);
    if s >> 24 == 255 {
        let b = s.to_le_bytes();
        for d in row.chunks_exact_mut(4) {
            d.copy_from_slice(&b);
        }
        return;
    }
    let inv = 255 - (s >> 24);
    // SAFETY: u8 -> u32 view; align_to_mut gives correctly aligned middle part only.
    let (pre, mid, post) = unsafe { row.align_to_mut::<u32>() };
    for d in pre.chunks_exact_mut(4).chain(post.chunks_exact_mut(4)) {
        let dv = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
        d.copy_from_slice(&s.wrapping_add(scale(dv, inv)).to_le_bytes());
    }
    for d in mid.iter_mut() {
        *d = s.wrapping_add(scale(u32::from_le(*d), inv)).to_le();
    }
}

/// atan2 to within ~0.0015 rad, several times quicker than libm's (it only picks a table slot).
#[inline(always)]
fn fast_atan2(y: f32, x: f32) -> f32 {
    let ax = x.abs();
    let ay = y.abs();
    let mx = ax.max(ay);
    if mx == 0.0 {
        return 0.0;
    }
    let a = ax.min(ay) / mx;
    let s = a * a;
    let mut r = ((-0.046_496_475 * s + 0.159_314_22) * s - 0.327_622_76) * s * a + a;
    if ay > ax {
        r = std::f32::consts::FRAC_PI_2 - r;
    }
    if x < 0.0 {
        r = std::f32::consts::PI - r;
    }
    if y < 0.0 { -r } else { r }
}

pub struct Target<'a> {
    pub data: &'a mut [u8],
    pub w: i32,
    pub h: i32,
}

impl Target<'_> {
    #[inline(always)]
    fn px(&mut self, x: i32, y: i32) -> Option<&mut [u8]> {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return None;
        }
        let i = ((y * self.w + x) * 4) as usize;
        self.data.get_mut(i..i + 4)
    }

    #[inline(always)]
    fn plot(&mut self, x: i32, y: i32, c: [u32; 4], cov: f32) {
        if cov <= 0.002 {
            return;
        }
        let k = (cov.min(1.0) * 256.0) as u32;
        if let Some(d) = self.px(x, y) {
            blend(d, c, k);
        }
    }

    /// A filled rounded rectangle (device pixels), two samples per row for smooth corners.
    pub fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c: [u32; 4]) {
        if w <= 0.0 || h <= 0.0 || c[3] == 0 {
            return;
        }
        let r = r.clamp(0.0, w.min(h) / 2.0);
        let (x1, y1) = (x + w, y + h);
        let jy0 = (y.floor() as i32).max(0);
        let jy1 = (y1.ceil() as i32).min(self.h);
        let span = |sy: f32| -> Option<(f32, f32)> {
            if sy < y || sy > y1 {
                return None;
            }
            let d = if sy < y + r {
                y + r - sy
            } else if sy > y1 - r {
                sy - (y1 - r)
            } else {
                0.0
            };
            let dx = if d > 0.0 {
                r - (r * r - d * d).max(0.0).sqrt()
            } else {
                0.0
            };
            Some((x + dx, x1 - dx))
        };
        for j in jy0..jy1 {
            let a = span(j as f32 + 0.25);
            let b = span(j as f32 + 0.75);
            let (lo, hi) = match (a, b) {
                (Some(a), Some(b)) => (a.0.min(b.0), a.1.max(b.1)),
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => continue,
            };
            let ilo = (lo.floor() as i32).max(0);
            let ihi = (hi.ceil() as i32).min(self.w);
            if ilo >= ihi {
                continue;
            }
            // the inside of both samples: full coverage
            let (fl, fr) = match (a, b) {
                (Some(a), Some(b)) => (a.0.max(b.0).ceil() as i32, a.1.min(b.1).floor() as i32),
                _ => (1, 0),
            };
            let cov = |i: i32, s: Option<(f32, f32)>| -> f32 {
                match s {
                    Some((l, r)) => (((i + 1) as f32).min(r) - (i as f32).max(l)).clamp(0.0, 1.0),
                    None => 0.0,
                }
            };
            let fl = fl.clamp(ilo, ihi);
            let fr = fr.clamp(fl, ihi);
            for i in ilo..fl {
                let k = 0.5 * (cov(i, a) + cov(i, b));
                self.plot(i, j, c, k);
            }
            if fr > fl {
                let s = ((j * self.w + fl) * 4) as usize;
                let e = ((j * self.w + fr) * 4) as usize;
                blend_run(&mut self.data[s..e], c);
            }
            for i in fr.max(fl)..ihi {
                let k = 0.5 * (cov(i, a) + cov(i, b));
                self.plot(i, j, c, k);
            }
        }
    }

    /// A ring stroke whose radius wobbles: R(t) = r + amp * sin(n t + phase), from `start` over `sweep`
    /// (radians, clockwise, 0 = 3 o'clock), with round ends. amp = 0 gives a plain ring / arc.
    #[allow(clippy::too_many_arguments)]
    pub fn polar_stroke(
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
        c: [u32; 4],
    ) {
        if sweep.abs() < 0.0005 || c[3] == 0 || r <= 0.0 {
            return;
        }
        let (start, sweep) = if sweep < 0.0 {
            (start + sweep, -sweep)
        } else {
            (start, sweep)
        };
        let full = sweep >= TAU - 1e-4;
        let hw = width / 2.0;
        let outer = r + amp.abs() + hw + 1.5;
        let inner = (r - amp.abs() - hw - 1.5).max(0.0);
        // a table of R and dR over the angle, so each pixel needs one atan2
        let k = ((TAU * outer * 2.0) as usize).clamp(64, 4096);
        let mut tr = Vec::with_capacity(k);
        let mut td = Vec::with_capacity(k);
        for i in 0..k {
            let t = i as f32 / k as f32 * TAU;
            let a = n * t + phase;
            tr.push(r + amp * a.sin());
            td.push(amp * n * a.cos());
        }
        let end_a = start + sweep;
        let ends = [
            {
                let rr = r + amp * (n * start + phase).sin();
                (cx + rr * start.cos(), cy + rr * start.sin())
            },
            {
                let rr = r + amp * (n * end_a + phase).sin();
                (cx + rr * end_a.cos(), cy + rr * end_a.sin())
            },
        ];
        let jy0 = ((cy - outer).floor() as i32).max(0);
        let jy1 = ((cy + outer).ceil() as i32).min(self.h);
        for j in jy0..jy1 {
            let dy = j as f32 + 0.5 - cy;
            if dy.abs() > outer {
                continue;
            }
            let xo = (outer * outer - dy * dy).max(0.0).sqrt();
            let xi = if dy.abs() < inner {
                (inner * inner - dy * dy).max(0.0).sqrt()
            } else {
                -1.0
            };
            let ranges: [(f32, f32); 2] = if xi > 0.0 {
                [(cx - xo, cx - xi), (cx + xi, cx + xo)]
            } else {
                [(cx - xo, cx + xo), (0.0, -1.0)]
            };
            for (a, b) in ranges {
                if b < a {
                    continue;
                }
                let i0 = (a.floor() as i32).max(0);
                let i1 = (b.ceil() as i32).min(self.w);
                for i in i0..i1 {
                    let dx = i as f32 + 0.5 - cx;
                    let rho = (dx * dx + dy * dy).sqrt();
                    let mut th = fast_atan2(dy, dx);
                    if th < 0.0 {
                        th += TAU;
                    }
                    let inside = full || {
                        let rel = (th - start).rem_euclid(TAU);
                        rel <= sweep
                    };
                    let dist = if inside {
                        let idx = ((th / TAU * k as f32) as usize).min(k - 1);
                        let rr = tr[idx];
                        let dd = td[idx] / rho.max(1.0);
                        (rho - rr).abs() / (1.0 + dd * dd).sqrt()
                    } else {
                        // round ends
                        let (px, py) = (i as f32 + 0.5, j as f32 + 0.5);
                        let d0 = ((px - ends[0].0).powi(2) + (py - ends[0].1).powi(2)).sqrt();
                        let d1 = ((px - ends[1].0).powi(2) + (py - ends[1].1).powi(2)).sqrt();
                        d0.min(d1)
                    };
                    let cov = hw + 0.5 - dist;
                    if cov > 0.0 {
                        self.plot(i, j, c, cov);
                    }
                }
            }
        }
    }

    /// A horizontal wave (or a straight line when amp = 0) from x0 to x1 with round ends.
    #[allow(clippy::too_many_arguments)]
    pub fn wave_h(
        &mut self,
        x0: f32,
        x1: f32,
        y: f32,
        amp: f32,
        wavelength: f32,
        phase: f32,
        width: f32,
        c: [u32; 4],
    ) {
        if x1 < x0 || c[3] == 0 {
            return;
        }
        let hw = width / 2.0;
        let k = TAU / wavelength.max(0.1);
        let fy = |x: f32| y + amp * ((x - x0) * k + phase).sin();
        let fd = |x: f32| amp * k * ((x - x0) * k + phase).cos();
        let i0 = ((x0 - hw - 1.0).floor() as i32).max(0);
        let i1 = ((x1 + hw + 1.0).ceil() as i32).min(self.w);
        let (ya, yb) = (fy(x0), fy(x1));
        for i in i0..i1 {
            let px = i as f32 + 0.5;
            let xc = px.clamp(x0, x1);
            let yc = fy(xc);
            let s = fd(xc);
            let norm = (1.0 + s * s).sqrt();
            let ext = hw * norm + 1.5;
            let j0 = ((yc - ext).floor() as i32).max(0);
            let j1 = ((yc + ext).ceil() as i32).min(self.h);
            for j in j0..j1 {
                let py = j as f32 + 0.5;
                let dist = if px < x0 {
                    ((px - x0).powi(2) + (py - ya).powi(2)).sqrt()
                } else if px > x1 {
                    ((px - x1).powi(2) + (py - yb).powi(2)).sqrt()
                } else {
                    (py - yc).abs() / norm
                };
                let cov = hw + 0.5 - dist;
                if cov > 0.0 {
                    self.plot(i, j, c, cov);
                }
            }
        }
    }

    /// A picture (already scaled to the mask's size) turned by `rot` around its centre, clipped by a coverage
    /// mask, at device (x0, y0). Bilinear sampling in fixed point; `alpha` 0..=256.
    pub fn rotated(
        &mut self,
        src: &tiny_skia::Pixmap,
        mask: &[u8],
        size: i32,
        x0: i32,
        y0: i32,
        rot: f32,
        alpha: u32,
    ) {
        let sw = src.width() as i32;
        let sh = src.height() as i32;
        let sd = src.data();
        let fetch = |xx: i32, yy: i32| -> u32 {
            let xx = xx.clamp(0, sw - 1);
            let yy = yy.clamp(0, sh - 1);
            let o = ((yy * sw + xx) * 4) as usize;
            u32::from_le_bytes([sd[o], sd[o + 1], sd[o + 2], sd[o + 3]])
        };
        let (s, co) = (-rot).sin_cos();
        let half = size as f32 / 2.0;
        let (scx, scy) = (sw as f32 / 2.0 - 0.5, sh as f32 / 2.0 - 0.5);
        let alpha = alpha.min(256);
        for j in 0..size {
            let y = y0 + j;
            if y < 0 || y >= self.h {
                continue;
            }
            let dy = j as f32 + 0.5 - half;
            // source position at i = 0 and its step along the row, in 1/256 px
            let mut sx = (((0.5 - half) * co - dy * s + scx) * 256.0) as i32;
            let mut sy = (((0.5 - half) * s + dy * co + scy) * 256.0) as i32;
            let stx = (co * 256.0) as i32;
            let sty = (s * 256.0) as i32;
            let row = (y * self.w) as usize;
            let mrow = (j * size) as usize;
            for i in 0..size {
                let m = mask[mrow + i as usize] as u32;
                let x = x0 + i;
                if m != 0 && x >= 0 && x < self.w {
                    let (ix, iy) = (sx >> 8, sy >> 8);
                    let (ax, ay) = ((sx & 255) as u32, (sy & 255) as u32);
                    let top =
                        scale(fetch(ix, iy), 255 - ax).wrapping_add(scale(fetch(ix + 1, iy), ax));
                    let bot = scale(fetch(ix, iy + 1), 255 - ax)
                        .wrapping_add(scale(fetch(ix + 1, iy + 1), ax));
                    let mut px = scale(top, 255 - ay).wrapping_add(scale(bot, ay));
                    let k = (m * alpha) >> 8;
                    if k < 255 {
                        px = scale(px, k);
                    }
                    let o = (row + x as usize) * 4;
                    let d = &mut self.data[o..o + 4];
                    let dv = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
                    d.copy_from_slice(&over(dv, px).to_le_bytes());
                }
                sx += stx;
                sy += sty;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shapes partly or wholly off the buffer, tiny, huge, negative: nothing may panic or write outside.
    #[test]
    fn no_panics_anywhere() {
        let (w, h) = (64, 48);
        let mut data = vec![0u8; (w * h * 4) as usize];
        let mut seed = 12345u32;
        let mut rnd = |a: f32, b: f32| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            a + (b - a) * ((seed >> 8) as f32 / (1u32 << 24) as f32)
        };
        let c = [100, 50, 20, 180];
        for _ in 0..3000 {
            let mut t = Target {
                data: &mut data,
                w,
                h,
            };
            let (x, y) = (rnd(-200.0, 260.0), rnd(-200.0, 250.0));
            let (rw, rh) = (rnd(-5.0, 300.0), rnd(-5.0, 300.0));
            t.rrect(x, y, rw, rh, rnd(-3.0, 80.0), c);
            t.polar_stroke(
                x,
                y,
                rnd(-5.0, 120.0),
                rnd(0.0, 6.0),
                14.0,
                rnd(0.0, 6.0),
                rnd(-7.0, 7.0),
                rnd(-7.0, 7.0),
                rnd(0.0, 8.0),
                c,
            );
            t.wave_h(
                x,
                x + rnd(-20.0, 300.0),
                y,
                rnd(0.0, 5.0),
                rnd(0.0, 30.0),
                0.3,
                rnd(0.0, 6.0),
                c,
            );
        }
        let src = tiny_skia::Pixmap::new(20, 20).unwrap();
        let mask = vec![255u8; 30 * 30];
        let mut t = Target {
            data: &mut data,
            w,
            h,
        };
        for (x, y) in [(-40, -40), (50, 40), (-10, 30), (60, -5)] {
            t.rotated(&src, &mask, 30, x, y, 0.7, 256);
        }
    }

    #[test]
    fn opaque_rect_fills_its_inside() {
        let (w, h) = (20, 20);
        let mut data = vec![0u8; (w * h * 4) as usize];
        let mut t = Target {
            data: &mut data,
            w,
            h,
        };
        t.rrect(2.0, 2.0, 16.0, 16.0, 4.0, [255, 0, 0, 255]);
        let px = |x: i32, y: i32| &data[((y * w + x) * 4) as usize..((y * w + x) * 4 + 4) as usize];
        assert_eq!(px(10, 10), &[255, 0, 0, 255]);
        assert_eq!(px(0, 0), &[0, 0, 0, 0]);
        // a rounded corner is not filled
        assert!(px(2, 2)[3] < 200);
    }
}
