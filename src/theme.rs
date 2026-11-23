//! The theme: one accent color (from the wallpaper or a named scheme) and the palettes the original derives
//! from it with HSL math (NPal.qml for panels, WPal.qml for desktop widgets).

use crate::color::{Rgba, rgba};

/// (key, name, accent) — the schemes of rice-colors; "auto" and "grey" come from the wallpaper.
pub const SCHEMES: &[(&str, &str, &str)] = &[
    ("auto", "Auto", ""),
    ("grey", "Grey", ""),
    ("navy", "Navy", "#4a6ad0"),
    ("mono", "Mono", "#d4d4d4"),
    ("slate", "Slate", "#9aa6b5"),
    ("stone", "Stone", "#b8aa9c"),
    ("steel", "Steel", "#8fa8b8"),
    ("ash", "Ash", "#a8a8a8"),
    ("ink", "Ink", "#707070"),
    ("ocean", "Ocean", "#4db5f0"),
    ("sky", "Sky", "#7fc0ff"),
    ("teal", "Teal", "#3cc7b6"),
    ("mint", "Mint", "#5fe3a8"),
    ("forest", "Forest", "#6fbf73"),
    ("lime", "Lime", "#b5d85a"),
    ("gold", "Gold", "#e6b84a"),
    ("sunset", "Sunset", "#ff9a6b"),
    ("ember", "Ember", "#ff6a63"),
    ("rose", "Rose", "#f48fa8"),
    ("orchid", "Orchid", "#b5a9ff"),
    ("violet", "Violet", "#a58bdc"),
];

/// Panel palette (control center, island cards, launcher, notifications, the bar): NPal "shell".
#[derive(Clone, Copy, Debug)]
pub struct Pal {
    pub primary: Rgba,
    pub ink_primary: Rgba,
    pub container: Rgba,
    pub ink_container: Rgba,
    pub surface: Rgba,
    pub card: Rgba,
    pub card_hi: Rgba,
    pub fg: Rgba,
    pub dim: Rgba,
    pub track: Rgba,
    pub line: Rgba,
    pub tertiary: Rgba,
    pub warn: Rgba,
}

/// Desktop widget palette (WPal.qml), for a dark or a light wallpaper behind the widget.
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub struct WPal {
    pub light: bool,
    pub fg: Rgba,
    pub fg2: Rgba,
    pub fg3: Rgba,
    pub fg4: Rgba,
    pub track: Rgba,
    pub chip: Rgba,
    pub chip_hover: Rgba,
    pub primary: Rgba,
    pub ink_on: Rgba,
    pub warn: Rgba,
    pub tertiary: Rgba,
    pub ink_tertiary: Rgba,
    pub shape_fill: Rgba,
    pub card: Rgba,
    pub edge: Rgba,
    pub radius: f32,
    pub shine: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WidgetStyle {
    Shadow,
    Google,
    Outline,
}

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub accent: Rgba,
    pub pal: Pal,
    pub wdark: WPal,
    pub wlight: WPal,
    pub accent_hi: Rgba,
}

fn hue_sat(acc: Rgba) -> (f32, f32, bool) {
    let (h, s, _) = acc.hsl();
    let mono = h < 0.0 || s < 0.1;
    (if h < 0.0 { 0.55 } else { h }, s, mono)
}

impl Theme {
    pub fn new(accent: Rgba, panel_alpha: f32, style: WidgetStyle, glass: bool, flat: bool) -> Theme {
        let (hue, s0, mono) = hue_sat(accent);
        let sat = if mono { 0.0 } else { s0.clamp(0.35, 0.85) };
        let a = if flat { 1.0 } else { panel_alpha };
        let surface = Rgba::hsla(hue, if mono { 0.0 } else { 0.08 }, 0.085, a);
        let card_base = Rgba::hsla(hue, if mono { 0.0 } else { 0.08 }, 0.085 * 1.75, 1.0);
        let card = card_base;
        let pal = Pal {
            primary: Rgba::hsla(hue, sat, 0.78, 1.0),
            ink_primary: Rgba::hsla(hue, sat.min(0.5), 0.13, 1.0),
            container: Rgba::hsla(hue, sat * 0.35, 0.25, 1.0),
            ink_container: Rgba::hsla(hue, sat * 0.6, 0.88, 1.0),
            surface,
            card,
            card_hi: card.lighter(1.3),
            fg: Rgba::hex("#ECEDEE").unwrap(),
            dim: Rgba::hex("#A9ADB2").unwrap(),
            track: rgba(1.0, 1.0, 1.0, 0.12),
            line: rgba(1.0, 1.0, 1.0, 0.06),
            tertiary: Rgba::hsla((hue + 0.5) % 1.0, 0.55, 0.72, 1.0),
            warn: Rgba::hex("#e06c75").unwrap(),
        };
        let (ah, as_, al) = accent.hsl();
        let accent_hi = Rgba::hsla(
            if ah < 0.0 { 0.0 } else { ah },
            if as_ < 0.08 { as_ } else { as_.clamp(0.42, 0.85) },
            al.clamp(0.7, 0.82),
            1.0,
        );
        Theme {
            accent,
            pal,
            wdark: wpal(accent, false, style, glass, flat),
            wlight: wpal(accent, true, style, glass, flat),
            accent_hi,
        }
    }

    pub fn w(&self, light: bool) -> &WPal {
        if light { &self.wlight } else { &self.wdark }
    }
}

fn wpal(acc: Rgba, light: bool, style: WidgetStyle, glass: bool, flat: bool) -> WPal {
    let (h, s, _) = acc.hsl();
    let hue = if h < 0.0 { 0.0 } else { h };
    let sat = if s < 0.08 { s } else { s.clamp(0.42, 0.9) };
    let google = style == WidgetStyle::Google;
    let outline = style == WidgetStyle::Outline;
    let fg = if light { Rgba::hex("#15151a").unwrap() } else { Rgba::hex("#F6F5F4").unwrap() };
    let k = 1.0;
    let card = if outline {
        crate::color::TRANSPARENT
    } else if google {
        if light {
            Rgba::hsla(hue, if sat < 0.08 { sat } else { sat * 0.6 }, 0.91, if flat { 1.0 } else if glass { 0.78 * k } else { 0.92 * k })
        } else {
            Rgba::hsla(hue, if sat < 0.08 { sat } else { sat * 0.34 }, 0.15, if flat { 1.0 } else if glass { 0.66 * k } else { 0.88 * k })
        }
    } else if light {
        if flat {
            rgba(0.95, 0.95, 0.96, 1.0)
        } else if glass {
            rgba(0.97, 0.97, 0.99, 0.55)
        } else {
            rgba(0.96, 0.96, 0.97, 0.86)
        }
    } else if flat {
        rgba(0.12, 0.12, 0.135, 1.0)
    } else if glass {
        rgba(0.10, 0.10, 0.115, 0.46)
    } else {
        rgba(0.17, 0.17, 0.19, 0.74)
    };
    let v = if light { 0.0 } else { 1.0 };
    WPal {
        light,
        fg,
        fg2: if light { Rgba::hex("#33333c").unwrap() } else { Rgba::hex("#c8c8cc").unwrap() },
        fg3: if light { Rgba::hex("#55555f").unwrap() } else { Rgba::hex("#8a8a8f").unwrap() },
        fg4: if light { Rgba::hex("#6d6d78").unwrap() } else { Rgba::hex("#6a6a70").unwrap() },
        track: rgba(v, v, v, if light { 0.14 } else { 0.13 }),
        chip: rgba(v, v, v, 0.08),
        chip_hover: rgba(v, v, v, 0.16),
        primary: if google {
            if light { Rgba::hsla(hue, sat, 0.34, 1.0) } else { Rgba::hsla(hue, sat, 0.78, 1.0) }
        } else {
            fg
        },
        ink_on: if google {
            if light { crate::color::WHITE } else { Rgba::hsla(hue, sat.min(0.5), 0.12, 1.0) }
        } else if light {
            Rgba::hex("#F6F5F4").unwrap()
        } else {
            Rgba::hex("#0d0d0d").unwrap()
        },
        warn: if light { Rgba::hex("#c0392b").unwrap() } else { Rgba::hex("#e06c75").unwrap() },
        tertiary: if light {
            Rgba::hsla((hue + 0.5) % 1.0, 0.5, 0.45, 1.0)
        } else {
            Rgba::hsla((hue + 0.5) % 1.0, 0.55, 0.72, 1.0)
        },
        ink_tertiary: if light { crate::color::WHITE } else { Rgba::hsla((hue + 0.5) % 1.0, 0.5, 0.14, 1.0) },
        shape_fill: if outline {
            rgba(v, v, v, 0.08)
        } else if light {
            Rgba::hsla(hue, sat * 0.4, 0.88, if flat { 1.0 } else { 0.86 })
        } else {
            Rgba::hsla(hue, sat * 0.3, 0.17, if flat { 1.0 } else { 0.8 })
        },
        card,
        edge: if outline {
            rgba(v, v, v, 0.42)
        } else if light {
            rgba(0.0, 0.0, 0.0, 0.09)
        } else {
            rgba(1.0, 1.0, 1.0, if glass { 0.10 } else { 0.06 })
        },
        radius: if google { 30.0 } else if outline { 20.0 } else { 26.0 },
        shine: style == WidgetStyle::Shadow && glass && !light,
    }
}

/// The accent of a wallpaper: the most present colorful hue, weighted by how colorful and how bright.
/// A black, white or grey picture gives a grey accent (like the original's "grey colors").
pub fn accent_from_pixels(px: &[[u8; 3]]) -> Rgba {
    let mut bins = [0f32; 36];
    let mut sum = [[0f32; 3]; 36];
    let mut grey = 0f32;
    let mut lsum = 0f32;
    for p in px {
        let c = rgba(p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0, 1.0);
        let (h, s, l) = c.hsl();
        lsum += l;
        let w = s * (1.0 - (2.0 * l - 1.0).abs());
        if h < 0.0 || w < 0.08 {
            grey += 1.0;
            continue;
        }
        let b = ((h * 36.0) as usize).min(35);
        bins[b] += w;
        sum[b][0] += c.r * w;
        sum[b][1] += c.g * w;
        sum[b][2] += c.b * w;
    }
    let n = px.len().max(1) as f32;
    // smooth over neighbours so a hue split across two bins still wins
    let mut best = 0;
    let mut bestv = 0.0;
    for i in 0..36 {
        let v = bins[i] + 0.5 * (bins[(i + 35) % 36] + bins[(i + 1) % 36]);
        if v > bestv {
            bestv = v;
            best = i;
        }
    }
    if bestv / n < 0.03 || grey / n > 0.93 {
        let l = (lsum / n).clamp(0.45, 0.75);
        return rgba(l, l, l, 1.0);
    }
    let w = bins[best].max(1e-6);
    let c = rgba(sum[best][0] / w, sum[best][1] / w, sum[best][2] / w, 1.0);
    let (h, s, l) = c.hsl();
    Rgba::hsla(h.max(0.0), s.clamp(0.25, 0.75), l.clamp(0.55, 0.75), 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grey_picture_gives_grey_accent() {
        let px = vec![[120u8, 120, 120]; 64 * 36];
        let a = accent_from_pixels(&px);
        assert!((a.r - a.g).abs() < 0.01 && (a.g - a.b).abs() < 0.01);
    }

    #[test]
    fn blue_picture_gives_blue_accent() {
        let mut px = vec![[10u8, 10, 20]; 64 * 36];
        for p in px.iter_mut().take(900) {
            *p = [40, 90, 220];
        }
        let a = accent_from_pixels(&px);
        assert!(a.b > a.r && a.b > a.g, "{a:?}");
    }
}
