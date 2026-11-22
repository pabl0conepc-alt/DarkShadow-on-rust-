//! The control center (the bell, SUPER + N): slides in from the right.
//! Control: brightness, volume and microphone, Wi-Fi and Bluetooth, quick switches, the music, notifications.
//! Overview: the time, a calendar, the weather and how busy the computer is.

use crate::color::{Rgba, rgba};
use crate::gfx::{Align, Canvas, Shape};
use crate::shell::SHADOW;
use crate::state::NetKind;
use crate::text::Face;
use crate::ui::{Act, Ctx, Hits, Out, Ui, icon_button, pill_slider};

pub fn draw(cv: &mut Canvas, ctx: &Ctx, ui: &mut Ui, hits: &mut Hits) -> Out {
    let now = ctx.now;
    let c = &ui.control;
    if c.rev.gone(now) {
        return Out { anim: false, input: Some(vec![]), done: true, damage: None };
    }
    let p = ctx.th.pal;
    let k = c.rev.value(now);
    let m = SHADOW as f32 - 8.0;
    let gap = 10.0;
    let (w, h) = (cv.w() - m - gap, cv.h() - 2.0 * gap);
    let x = m + (1.0 - k) * (w * 0.35);
    let y = gap;
    cv.alpha = k;
    if ctx.cfg.theme.shadows {
        cv.shadow(x, y + 4.0, w, h, 32.0, 16.0, 0.7);
    }
    cv.rrect(x, y, w, h, 32.0, p.surface);
    cv.rrect_stroke(x, y, w, h, 32.0, 1.0, p.line);

    // header: close, the two tabs, settings
    icon_button(cv, hits, x + 36.0, y + 36.0, 20.0, "close", p.fg, p.card, p.card_hi, Act::ToggleControl);
    let tabs = ["Control", "Overview"];
    let tw: Vec<f32> = tabs.iter().map(|t| cv.measure(t, Face::Semi, 13.0) + 30.0).collect();
    let total = tw.iter().sum::<f32>() + 4.0;
    let mut tx = x + (w - total) / 2.0;
    for (i, t) in tabs.iter().enumerate() {
        let on = c.tab as usize == i;
        let hov = hits.add(tx, y + 16.0, tw[i], 40.0, Act::ControlTab(i as u8));
        if on {
            cv.rrect(tx, y + 16.0, tw[i], 40.0, 20.0, p.primary);
        } else if hov {
            cv.rrect(tx, y + 16.0, tw[i], 40.0, 20.0, rgba(1.0, 1.0, 1.0, 0.06));
        }
        cv.text(tx + tw[i] / 2.0, y + 36.0, t, Face::Semi, 13.0, if on { p.ink_primary } else { p.dim }, Align::Center);
        tx += tw[i] + 4.0;
    }
    icon_button(cv, hits, x + w - 36.0, y + 36.0, 20.0, "settings", p.fg, p.card, p.card_hi, Act::Settings);

    let (bx, bw) = (x + 16.0, w - 32.0);
    let mut by = y + 72.0;
    let mut anim = c.rev.animating(now);
    if c.tab == 1 {
        anim |= overview(cv, ctx, hits, bx, by, bw);
    } else {
        // sliders
        let st = ctx.st;
        if st.bright.ok {
            pill_slider(cv, hits, ctx.th, bx, by, bw, 46.0, st.bright.pct, "brightness_medium", false, Act::Bright);
            hits.with_scroll(Act::Bright);
            by += 54.0;
        }
        if st.audio.ok {
            let vi = if st.audio.muted { "volume_off" } else { "volume_up" };
            pill_slider(cv, hits, ctx.th, bx, by, bw - 54.0, 46.0, st.audio.vol, vi, st.audio.muted, Act::Volume);
            hits.with_scroll(Act::Volume);
            icon_button(cv, hits, bx + bw - 23.0, by + 23.0, 23.0, vi, if st.audio.muted { p.ink_primary } else { p.fg }, if st.audio.muted { p.primary } else { p.card }, p.card_hi, Act::Mute);
            by += 54.0;
            let mi = if st.audio.mic_muted { "mic_off" } else { "mic" };
            pill_slider(cv, hits, ctx.th, bx, by, bw - 54.0, 46.0, st.audio.mic, mi, st.audio.mic_muted, Act::Mic);
            icon_button(cv, hits, bx + bw - 23.0, by + 23.0, 23.0, mi, if st.audio.mic_muted { p.ink_primary } else { p.fg }, if st.audio.mic_muted { p.primary } else { p.card }, p.card_hi, Act::MicMute);
            by += 54.0;
        }
        by += 4.0;
        // Wi-Fi and Bluetooth
        let half = (bw - 8.0) / 2.0;
        let wifi_sub = match st.net.kind {
            NetKind::Wifi => st.net.ssid.clone(),
            NetKind::Wired => "Ethernet".into(),
            NetKind::None => {
                if st.net.wifi_on {
                    "Not connected".into()
                } else {
                    "Off".into()
                }
            }
        };
        wide(cv, ctx, hits, bx, by, half, st.net.icon(), "Wi-Fi", &wifi_sub, st.net.wifi_on, Act::Wifi);
        let bt_sub = if !st.bt.present {
            "No adapter".to_string()
        } else if !st.bt.powered {
            "Off".into()
        } else if st.bt.connected.is_empty() {
            "On".into()
        } else {
            st.bt.connected.join(", ")
        };
        let bt_icon = if st.bt.powered { "bluetooth" } else { "bluetooth_disabled" };
        wide(cv, ctx, hits, bx + half + 8.0, by, half, bt_icon, "Bluetooth", &bt_sub, st.bt.powered, Act::Bluetooth);
        by += 72.0;
        // quick switches
        let tiles = [
            ("do_not_disturb_on", "Do Not Disturb", st.toggles.dnd, Act::Dnd),
            ("nightlight", "Night light", st.toggles.nightlight, Act::NightLight),
            ("coffee", "Keep awake", st.toggles.awake, Act::Awake),
            ("sports_esports", "Gaming", st.toggles.gaming, Act::Gaming),
        ];
        let tw = (bw - 3.0 * 8.0) / 4.0;
        for (i, (icon, label, on, act)) in tiles.iter().enumerate() {
            tile(cv, ctx, hits, bx + i as f32 * (tw + 8.0), by, tw, icon, label, *on, act.clone());
        }
        by += 82.0;
        // the music
        if st.media.active() {
            anim |= music(cv, ctx, hits, bx, by, bw);
            by += 150.0;
        }
        // notifications
        let list = &st.notifs.list;
        cv.text(bx + 6.0, by + 16.0, "Notifications", Face::Semi, 15.0, p.fg, Align::Left);
        if !list.is_empty() {
            let lw = cv.measure("Clear all", Face::Semi, 12.0) + 24.0;
            let hov = hits.add(bx + bw - lw, by + 2.0, lw, 28.0, Act::NotifClear);
            cv.rrect(bx + bw - lw, by + 2.0, lw, 28.0, 14.0, if hov { p.card_hi } else { p.card });
            cv.text(bx + bw - lw / 2.0, by + 16.0, "Clear all", Face::Semi, 12.0, p.fg, Align::Center);
        }
        by += 40.0;
        let bottom = y + h - 12.0;
        if list.is_empty() {
            let cy = by + ((bottom - by) / 2.0).min(90.0);
            cv.shape(Shape::Cookie(9), x + w / 2.0, cy - 10.0, 30.0, 0.0, p.card);
            cv.icon(if st.toggles.dnd { "notifications_off" } else { "notifications" }, x + w / 2.0, cy - 10.0, 26.0, p.dim);
            cv.text(x + w / 2.0, cy + 40.0, if st.toggles.dnd { "Do Not Disturb is on" } else { "Nothing new" }, Face::Medium, 13.0, p.dim, Align::Center);
        } else {
            let max_scroll = (list.len() as f32 * 80.0 - (bottom - by)).max(0.0);
            let scroll = ui.control.scroll.min(max_scroll);
            ui.control.scroll = scroll;
            for (i, n) in list.iter().enumerate() {
                let ny = by + i as f32 * 80.0 - scroll;
                if ny < by - 1.0 || ny + 72.0 > bottom {
                    continue;
                }
                notif_row(cv, ctx, hits, bx, ny, bw, n);
            }
        }
    }
    cv.alpha = 1.0;
    Out { anim, input: Some(vec![(x, y, w, h)]), done: false, damage: None }
}

#[allow(clippy::too_many_arguments)]
fn wide(cv: &mut Canvas, ctx: &Ctx, hits: &mut Hits, x: f32, y: f32, w: f32, icon: &str, label: &str, sub: &str, on: bool, act: Act) {
    let p = ctx.th.pal;
    let hov = hits.add(x, y, w, 64.0, act);
    let pressed = hits.is_pressed_last();
    let r = if pressed { 16.0 } else { 24.0 };
    let bg = if on { p.primary } else if hov { p.card_hi } else { p.card };
    cv.rrect(x, y, w, 64.0, r, bg);
    let (fg, dim) = if on { (p.ink_primary, p.ink_primary.alpha(0.7)) } else { (p.fg, p.dim) };
    cv.icon(icon, x + 28.0, y + 32.0, 22.0, fg);
    cv.text_max(x + 52.0, y + 23.0, label, Face::Semi, 13.0, fg, Align::Left, w - 62.0);
    cv.text_max(x + 52.0, y + 43.0, sub, Face::Regular, 11.0, dim, Align::Left, w - 62.0);
}

#[allow(clippy::too_many_arguments)]
fn tile(cv: &mut Canvas, ctx: &Ctx, hits: &mut Hits, x: f32, y: f32, w: f32, icon: &str, label: &str, on: bool, act: Act) {
    let p = ctx.th.pal;
    let hov = hits.add(x, y, w, 70.0, act);
    let pressed = hits.is_pressed_last();
    // the shape closes in when it is on
    let r = if pressed { 14.0 } else if on { 22.0 } else { 35.0 };
    let bg = if on { p.primary } else if hov { p.card_hi } else { p.card };
    cv.rrect(x, y, w, 70.0, r, bg);
    let fg = if on { p.ink_primary } else { p.fg };
    cv.icon(icon, x + w / 2.0, y + 26.0, 22.0, fg);
    cv.text_max(x + w / 2.0, y + 52.0, label, Face::Medium, 10.5, if on { fg } else { p.dim }, Align::Center, w - 10.0);
}

fn music(cv: &mut Canvas, ctx: &Ctx, hits: &mut Hits, x: f32, y: f32, w: f32) -> bool {
    let p = ctx.th.pal;
    let m = &ctx.st.media;
    let now = ctx.now;
    let h = 140.0;
    cv.rrect(x, y, w, h, 28.0, p.card);
    let (cx, cy, r) = (x + 54.0, y + 54.0, 36.0);
    let rot = if m.playing { crate::ui::secs(now) * 0.25 } else { 0.0 };
    // the cover as a cookie in a wavy ring
    if ctx.cfg.theme.wavy {
        cv.wavy_arc(cx, cy, r + 7.0, 1.8, 12.0, -crate::ui::secs(now) * 1.5, -std::f32::consts::FRAC_PI_2, std::f32::consts::TAU * m.fraction(now), 2.5, p.primary);
    }
    match &m.art {
        Some(img) => cv.image_shape(img, Shape::Cookie(9), cx, cy, r, rot),
        None => {
            cv.shape(Shape::Cookie(9), cx, cy, r, rot, p.container);
            cv.icon("music_note", cx, cy, 26.0, p.ink_container);
        }
    }
    let tx = x + 108.0;
    let tw = w - 108.0 - 16.0;
    cv.text_max(tx, y + 38.0, &m.title, Face::Semi, 14.0, p.fg, Align::Left, tw);
    let sub = if m.artist.is_empty() { &m.identity } else { &m.artist };
    cv.text_max(tx, y + 58.0, sub, Face::Regular, 12.0, p.dim, Align::Left, tw);
    let frac = ctx.seek.unwrap_or(m.fraction(now));
    crate::ui::card::seek_line(cv, ctx, hits, x + 20.0, x + w - 20.0, y + 98.0, frac, m.playing);
    let by = y + 124.0;
    let mid = x + w / 2.0;
    let none = rgba(0.0, 0.0, 0.0, 0.0);
    icon_button(cv, hits, mid - 100.0, by, 13.0, "shuffle", p.dim, none, p.card_hi, Act::None);
    icon_button(cv, hits, mid - 52.0, by, 15.0, "skip_previous", p.fg, none, p.card_hi, Act::Prev);
    let hov = hits.add(mid - 22.0, by - 14.0, 44.0, 28.0, Act::PlayPause);
    cv.rrect(mid - 22.0, by - 14.0, 44.0, 28.0, 14.0, if hov { p.primary.lighter(1.08) } else { p.primary });
    cv.icon(if m.playing { "pause" } else { "play_arrow" }, mid, by, 19.0, p.ink_primary);
    icon_button(cv, hits, mid + 52.0, by, 15.0, "skip_next", p.fg, none, p.card_hi, Act::Next);
    icon_button(cv, hits, mid + 100.0, by, 13.0, "repeat", p.dim, none, p.card_hi, Act::None);
    m.playing
}

#[allow(clippy::too_many_arguments)]
fn notif_row(cv: &mut Canvas, ctx: &Ctx, hits: &mut Hits, x: f32, y: f32, w: f32, n: &crate::state::Notif) {
    let p = ctx.th.pal;
    let hov = hits.add(x, y, w, 72.0, Act::NotifOpen(n.id));
    cv.rrect(x, y, w, 72.0, 22.0, if hov { p.card_hi } else { p.card });
    let (ix, iy) = (x + 34.0, y + 36.0);
    match &n.image {
        Some(img) => cv.image_shape(img, Shape::Cookie(7), ix, iy, 20.0, 0.0),
        None => {
            cv.shape(Shape::Cookie(7), ix, iy, 20.0, 0.0, p.container);
            cv.icon("notifications", ix, iy, 18.0, p.ink_container);
        }
    }
    let tx = x + 66.0;
    let tw = w - 66.0 - 44.0;
    let ago = n.at.elapsed().as_secs();
    let when = if ago < 60 {
        "now".to_string()
    } else if ago < 3600 {
        format!("{} min", ago / 60)
    } else {
        format!("{} h", ago / 3600)
    };
    cv.text_max(tx, y + 18.0, &format!("{} · {when}", if n.app.is_empty() { "App" } else { &n.app }), Face::Medium, 10.5, p.dim, Align::Left, tw);
    cv.text_max(tx, y + 37.0, &n.summary, Face::Semi, 13.0, p.fg, Align::Left, tw);
    cv.text_max(tx, y + 55.0, &n.body.replace('\n', " "), Face::Regular, 12.0, p.dim, Align::Left, tw);
    icon_button(cv, hits, x + w - 24.0, y + 24.0, 13.0, "close", p.dim, rgba(0.0, 0.0, 0.0, 0.0), p.surface, Act::NotifDismiss(n.id));
}

fn overview(cv: &mut Canvas, ctx: &Ctx, hits: &mut Hits, x: f32, y: f32, w: f32) -> bool {
    let p = ctx.th.pal;
    let tm = crate::clock::now();
    let _ = hits;
    cv.text(x + w / 2.0, y + 50.0, &format!("{:02}:{:02}", tm.hour, tm.min), Face::Display, 84.0, p.fg, Align::Center);
    cv.text(
        x + w / 2.0,
        y + 112.0,
        &format!("{}, {} {}", crate::ui::weekday(tm.wday), tm.mday, crate::ui::month(tm.mon)),
        Face::Medium,
        14.0,
        p.dim,
        Align::Center,
    );
    // calendar card
    let cy = y + 140.0;
    cv.rrect(x, cy, w, 250.0, 28.0, p.card);
    crate::ui::card::calendar(cv, ctx, x + 16.0, cy + 14.0, w - 32.0, &tm, p.fg, p.dim, p.primary, p.ink_primary);
    // weather
    let wy = cy + 262.0;
    let wst = &ctx.st.weather;
    cv.rrect(x, wy, w, 84.0, 28.0, p.card);
    if wst.ok {
        cv.shape(Shape::Cookie(7), x + 44.0, wy + 42.0, 26.0, 0.0, p.container);
        cv.icon(wst.icon, x + 44.0, wy + 42.0, 28.0, p.ink_container);
        cv.text(x + 84.0, wy + 32.0, &format!("{:.0}°{}  {}", wst.temp, wst.unit, wst.desc), Face::Semi, 15.0, p.fg, Align::Left);
        cv.text_max(
            x + 84.0,
            wy + 55.0,
            &format!("{} · feels {:.0}° · {}% humidity", wst.city, wst.feels, wst.humidity),
            Face::Regular,
            12.0,
            p.dim,
            Align::Left,
            w - 100.0,
        );
    } else {
        cv.text(x + w / 2.0, wy + 42.0, "Weather loading…", Face::Regular, 13.0, p.dim, Align::Center);
    }
    // the computer
    let sy = wy + 96.0;
    cv.rrect(x, sy, w, 128.0, 28.0, p.card);
    let s = &ctx.st.sys;
    let items: [(&str, f32); 3] = [("CPU", s.cpu), ("RAM", s.ram), (if s.gpu.is_some() { "GPU" } else { "Disk" }, s.gpu.unwrap_or(s.disk))];
    let cell = w / 3.0;
    for (i, (label, v)) in items.iter().enumerate() {
        let cx = x + cell * (i as f32 + 0.5);
        let ccy = sy + 56.0;
        cv.ring(cx, ccy, 34.0, 6.0, p.track);
        cv.wavy_arc(cx, ccy, 34.0, 2.0, 12.0, -crate::ui::secs(ctx.now), -std::f32::consts::FRAC_PI_2, std::f32::consts::TAU * v.clamp(0.0, 1.0), 6.0, p.primary);
        cv.text(cx, ccy, &format!("{}%", (v * 100.0).round()), Face::Numbers, 14.0, p.fg, Align::Center);
        cv.text(cx, sy + 110.0, label, Face::Medium, 11.0, p.dim, Align::Center);
    }
    true
}

#[allow(dead_code)]
fn _unused(_: Rgba) {}
