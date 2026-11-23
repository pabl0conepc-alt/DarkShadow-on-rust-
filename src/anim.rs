//! Motion: Material 3 easing curves, tweens and a small spring.

use std::time::{Duration, Instant};

/// cubic-bezier(x1, y1, x2, y2) evaluated at time t (0..1).
pub fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t <= 0.0 || t >= 1.0 {
        return t;
    }
    let bx = |s: f32| 3.0 * (1.0 - s) * (1.0 - s) * s * x1 + 3.0 * (1.0 - s) * s * s * x2 + s * s * s;
    let by = |s: f32| 3.0 * (1.0 - s) * (1.0 - s) * s * y1 + 3.0 * (1.0 - s) * s * s * y2 + s * s * s;
    // Newton then bisection for the s whose x is t
    let mut s = t;
    for _ in 0..6 {
        let x = bx(s) - t;
        let d = 3.0 * (1.0 - s) * (1.0 - s) * x1 + 6.0 * (1.0 - s) * s * (x2 - x1) + 3.0 * s * s * (1.0 - x2);
        if d.abs() < 1e-5 {
            break;
        }
        s = (s - x / d).clamp(0.0, 1.0);
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..12 {
        if (bx(s) - t).abs() < 1e-4 {
            break;
        }
        if bx(s) < t {
            lo = s
        } else {
            hi = s
        }
        s = (lo + hi) / 2.0;
    }
    by(s)
}

/// emphasized-decelerate: things coming in
pub fn decel(t: f32) -> f32 {
    bezier(0.05, 0.7, 0.1, 1.0, t)
}
/// emphasized-accelerate: things going away
pub fn accel(t: f32) -> f32 {
    bezier(0.3, 0.0, 0.8, 0.15, t)
}
/// standard
pub fn std_ease(t: f32) -> f32 {
    bezier(0.2, 0.0, 0.0, 1.0, t)
}
/// a little overshoot (spatial, expressive)
pub fn overshoot(t: f32) -> f32 {
    bezier(0.34, 1.36, 0.64, 1.0, t)
}

pub fn progress(start: Instant, now: Instant, dur_ms: u64) -> f32 {
    let e = now.saturating_duration_since(start).as_secs_f32() * 1000.0;
    (e / dur_ms.max(1) as f32).clamp(0.0, 1.0)
}

/// An open/close transition: 0 hidden .. 1 shown.
#[derive(Clone, Copy, Debug)]
pub struct Reveal {
    pub open: bool,
    pub since: Instant,
    pub from: f32,
    pub in_ms: u64,
    pub out_ms: u64,
}

impl Reveal {
    pub fn new(in_ms: u64, out_ms: u64) -> Self {
        Reveal { open: false, since: Instant::now() - Duration::from_secs(10), from: 0.0, in_ms, out_ms }
    }
    /// raw 0..1 position (linear in time)
    pub fn raw(&self, now: Instant) -> f32 {
        if self.open {
            let p = progress(self.since, now, (self.in_ms as f32 * (1.0 - self.from)) as u64);
            self.from + (1.0 - self.from) * p
        } else {
            let p = progress(self.since, now, (self.out_ms as f32 * self.from) as u64);
            self.from * (1.0 - p)
        }
    }
    /// eased: decelerate in, accelerate out
    pub fn value(&self, now: Instant) -> f32 {
        let r = self.raw(now);
        if self.open { decel(r) } else { 1.0 - accel(1.0 - r) }
    }
    pub fn set(&mut self, open: bool, now: Instant) {
        if open == self.open {
            return;
        }
        self.from = self.raw(now);
        self.open = open;
        self.since = now;
    }
    pub fn animating(&self, now: Instant) -> bool {
        let r = self.raw(now);
        if self.open { r < 1.0 } else { r > 0.0 }
    }
    pub fn gone(&self, now: Instant) -> bool {
        !self.open && self.raw(now) <= 0.0
    }
}

/// A value that glides to its target over `ms` (re-targeting keeps it smooth).
#[derive(Clone, Copy, Debug)]
pub struct Glide {
    pub from: f32,
    pub to: f32,
    pub since: Instant,
    pub ms: u64,
}

impl Glide {
    pub fn new(v: f32, ms: u64) -> Self {
        Glide { from: v, to: v, since: Instant::now() - Duration::from_secs(10), ms }
    }
    pub fn get(&self, now: Instant) -> f32 {
        let t = std_ease(progress(self.since, now, self.ms));
        self.from + (self.to - self.from) * t
    }
    pub fn set(&mut self, v: f32, now: Instant) {
        if (v - self.to).abs() < 0.001 {
            return;
        }
        self.from = self.get(now);
        self.to = v;
        self.since = now;
    }
    pub fn jump(&mut self, v: f32) {
        self.from = v;
        self.to = v;
    }
    pub fn moving(&self, now: Instant) -> bool {
        progress(self.since, now, self.ms) < 1.0 && (self.from - self.to).abs() > 0.001
    }
}

/// A critically-damped-ish spring for the island's size (Material 3 spatial motion).
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub x: f32,
    pub v: f32,
    pub target: f32,
    pub last: Instant,
    pub k: f32,
    pub d: f32,
}

impl Spring {
    pub fn new(x: f32) -> Self {
        Spring { x, v: 0.0, target: x, last: Instant::now(), k: 380.0, d: 30.0 }
    }
    pub fn step(&mut self, now: Instant) -> f32 {
        let mut dt = now.saturating_duration_since(self.last).as_secs_f32().min(0.05);
        self.last = now;
        while dt > 0.0 {
            let h = dt.min(1.0 / 240.0);
            let a = self.k * (self.target - self.x) - self.d * self.v;
            self.v += a * h;
            self.x += self.v * h;
            dt -= h;
        }
        if self.settled() {
            self.x = self.target;
            self.v = 0.0;
        }
        self.x
    }
    pub fn settled(&self) -> bool {
        (self.target - self.x).abs() < 0.3 && self.v.abs() < 2.0
    }
    pub fn set(&mut self, t: f32, now: Instant) {
        if self.settled() {
            self.last = now;
        }
        self.target = t;
    }
}
