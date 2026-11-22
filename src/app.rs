//! The Wayland side: one connection, one event loop, layer surfaces drawn on the CPU into shared memory.
//!
//! Why this is light:
//! - a surface is only redrawn when a topic it shows changed, or while it animates (frame callbacks, so a
//!   hidden surface costs nothing);
//! - a surface that has been still for a moment gives its pixel memory back (the compositor keeps a copy);
//! - panels exist only while open; closing destroys the surface and returns the memory to the system.

use std::time::{Duration, Instant};

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData, Region},
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{
            EventLoop, LoopHandle,
            timer::{TimeoutAction, Timer},
        },
        calloop_wayland_source::WaylandSource,
        client::{
            Connection, QueueHandle,
            globals::registry_queue_init,
            protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
        },
        protocols::wp::cursor_shape::v1::client::{
            wp_cursor_shape_device_v1::{Shape as CursorShape, WpCursorShapeDeviceV1},
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler, cursor_shape::CursorShapeManager},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use tiny_skia::PixmapMut;

use crate::config::{Config, Saved};
use crate::gfx::{Canvas, Gfx};
use crate::state::State;
use crate::theme::{Theme, WidgetStyle};
use crate::ui::{self, Act, Ctx, Hit, HitKind, Hits, Kind, Out, Ui};

pub struct Spec {
    pub kind: Kind,
    pub layer: Layer,
    pub ns: &'static str,
    pub anchor: Anchor,
    pub size: (u32, u32),
    pub exclusive: i32,
    /// top, right, bottom, left
    pub margin: (i32, i32, i32, i32),
    pub kb: KeyboardInteractivity,
    pub output: Option<wl_output::WlOutput>,
    pub topics: u32,
    /// frame-rate cap while animating (0: as fast as the screen)
    pub fps: u32,
}

pub struct Surf {
    pub kind: Kind,
    pub layer: LayerSurface,
    pub output: Option<wl_output::WlOutput>,
    pool: Option<SlotPool>,
    last_buf: Option<smithay_client_toolkit::shm::slot::Buffer>,
    pub w: u32,
    pub h: u32,
    pub scale: i32,
    configured: bool,
    pub dirty: bool,
    waiting_frame: bool,
    pub anim: bool,
    fps: u32,
    last_draw: Instant,
    timer_armed: bool,
    pub hits: Vec<Hit>,
    pub hover: Option<u32>,
    pub pressed: Option<u32>,
    pub topics: u32,
    input_sig: u64,
    pub closing: bool,
    pub draws: u32,
    pub pool_len: usize,
    pub draw_us: u64,
}

#[derive(Default)]
struct Ptr {
    surf: Option<wl_surface::WlSurface>,
    x: f32,
    y: f32,
    /// (surface kind, hit id, button)
    pressed: Option<(Kind, u32, u32)>,
    serial: u32,
}

pub struct App {
    pub qh: QueueHandle<App>,
    pub lh: LoopHandle<'static, App>,
    registry: RegistryState,
    seat: SeatState,
    pub outputs: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    abgr: bool,
    cursor_mgr: Option<CursorShapeManager>,
    cursor_dev: Option<WpCursorShapeDeviceV1>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pub surfs: Vec<Surf>,
    pub gfx: Gfx,
    pub cfg: Config,
    pub cfg_err: Option<String>,
    pub saved: Saved,
    pub st: State,
    pub ui: Ui,
    pub theme: Theme,
    pub svc: crate::svc::Services,
    ptr: Ptr,
    pub kb_focus: Option<Kind>,
    pub mods: Modifiers,
    pub exit: bool,
    pub started: Instant,
    trim_armed: bool,
}

pub fn make_theme(cfg: &Config, st: &State) -> Theme {
    let style = match cfg.theme.widget_style.as_str() {
        "shadow" | "darkshadow" => WidgetStyle::Shadow,
        "outline" => WidgetStyle::Outline,
        _ => WidgetStyle::Google,
    };
    let scheme = cfg.theme.scheme.as_str();
    let accent = if let Some(c) = crate::color::Rgba::hex(scheme) {
        c
    } else if let Some((_, _, hex)) = crate::theme::SCHEMES.iter().find(|s| s.0 == scheme && !s.2.is_empty()) {
        crate::color::Rgba::hex(hex).unwrap()
    } else {
        let a = st.wall.accent.unwrap_or(crate::color::Rgba::hex("#9ec9bd").unwrap());
        if scheme == "grey" {
            let l = a.luma().clamp(0.5, 0.75);
            crate::color::rgba(l, l, l, 1.0)
        } else {
            a
        }
    };
    let gaming = cfg.performance.gaming || st.toggles.gaming;
    Theme::new(accent, cfg.theme.panel_alpha, style, cfg.theme.blur && !gaming, cfg.theme.flat || gaming)
}

impl App {
    // ------------------------------------------------------------------ surfaces

    pub fn surf(&self, kind: Kind) -> Option<&Surf> {
        self.surfs.iter().find(|s| s.kind == kind)
    }
    pub fn surf_mut(&mut self, kind: Kind) -> Option<&mut Surf> {
        self.surfs.iter_mut().find(|s| s.kind == kind)
    }
    pub fn has(&self, kind: Kind) -> bool {
        self.surfs.iter().any(|s| s.kind == kind && !s.closing)
    }

    pub fn open(&mut self, sp: Spec) {
        if let Some(s) = self.surf_mut(sp.kind) {
            s.closing = false;
            s.dirty = true;
            self.schedule_flush();
            return;
        }
        let wl = self.compositor.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(&self.qh, wl, sp.layer, Some(sp.ns), sp.output.as_ref());
        layer.set_anchor(sp.anchor);
        layer.set_size(sp.size.0, sp.size.1);
        layer.set_exclusive_zone(sp.exclusive);
        layer.set_margin(sp.margin.0, sp.margin.1, sp.margin.2, sp.margin.3);
        layer.set_keyboard_interactivity(sp.kb);
        let scale = sp
            .output
            .as_ref()
            .and_then(|o| self.outputs.info(o))
            .map(|i| i.scale_factor)
            .unwrap_or(1)
            .max(1);
        layer.wl_surface().set_buffer_scale(scale);
        // nothing takes the pointer until the first draw says where
        if let Ok(r) = Region::new(&self.compositor) {
            layer.wl_surface().set_input_region(Some(r.wl_region()));
        }
        layer.commit();
        self.surfs.push(Surf {
            kind: sp.kind,
            layer,
            output: sp.output,
            pool: None,
            last_buf: None,
            w: sp.size.0.max(1),
            h: sp.size.1.max(1),
            scale,
            configured: false,
            dirty: true,
            waiting_frame: false,
            anim: false,
            fps: sp.fps,
            last_draw: Instant::now() - Duration::from_secs(5),
            timer_armed: false,
            hits: Vec::new(),
            hover: None,
            pressed: None,
            topics: sp.topics,
            input_sig: 1,
            closing: false,
            draws: 0,
            pool_len: 0,
            draw_us: 0,
        });
    }

    pub fn destroy(&mut self, kind: Kind) {
        if let Some(i) = self.surfs.iter().position(|s| s.kind == kind) {
            let s = self.surfs.remove(i);
            if self.ptr.surf.as_ref() == Some(s.layer.wl_surface()) {
                self.ptr.surf = None;
                self.ptr.pressed = None;
            }
            if self.kb_focus == Some(kind) {
                self.kb_focus = None;
            }
            drop(s);
            if kind == Kind::Picker {
                self.ui.picker.thumbs.clear();
                self.ui.picker.thumbs.shrink_to_fit();
                self.ui.picker.list.clear();
            }
            if kind == Kind::Launcher {
                // app icons stay cached only while the launcher is open
                self.svc.app_icons.clear();
                self.svc.app_icons.shrink_to_fit();
            }
            self.arm_trim(Duration::from_millis(400));
        }
    }

    /// Mark surfaces showing `topics` as dirty and draw them on the next turn of the loop.
    pub fn mark(&mut self, topics: u32) {
        let mut any = false;
        for s in &mut self.surfs {
            if s.topics & topics != 0 {
                s.dirty = true;
                any = true;
            }
        }
        if any {
            self.schedule_flush();
        }
    }

    pub fn mark_kind(&mut self, kind: Kind) {
        if let Some(s) = self.surf_mut(kind) {
            s.dirty = true;
        }
        self.schedule_flush();
    }

    pub fn schedule_flush(&mut self) {}

    /// Draw every surface that is dirty and not waiting for the compositor.
    pub fn flush(&mut self) {
        for i in 0..self.surfs.len() {
            if i >= self.surfs.len() {
                break;
            }
            let s = &self.surfs[i];
            if s.configured && s.dirty && !s.waiting_frame {
                self.draw(i);
            }
        }
        for name in std::mem::take(&mut self.ui.want_icons) {
            crate::svc::images::load_app_icon(self, name);
        }
        for i in std::mem::take(&mut self.ui.want_thumbs) {
            if let Some(p) = self.ui.picker.list.get(i).cloned() {
                crate::svc::images::load_thumb(self, i, p);
            }
        }
        if std::mem::take(&mut self.ui.wall_redecode) {
            let p = std::path::PathBuf::from(&self.st.wall.path);
            if p.is_file() {
                crate::shell::set_wallpaper(self, p, false);
            }
        }
        // panels that finished closing
        let done: Vec<Kind> = self.surfs.iter().filter(|s| s.closing && !s.anim).map(|s| s.kind).collect();
        for k in done {
            self.destroy(k);
        }
    }

    fn screen_size(&self) -> (f32, f32) {
        if let Some(o) = self.svc.main_output.as_ref().and_then(|o| self.outputs.info(o))
            && let Some((w, h)) = o.logical_size
        {
            return (w as f32, h as f32);
        }
        (1920.0, 1080.0)
    }

    fn draw(&mut self, i: usize) {
        let now = Instant::now();
        let (sw, sh) = self.screen_size();
        let abgr = self.abgr;
        let qh = self.qh.clone();
        let App { surfs, gfx, cfg, st, ui, theme, shm, compositor, svc, .. } = self;
        let s = &mut surfs[i];
        // frame-rate cap for decorative motion
        if s.anim && s.fps > 0 {
            let min = Duration::from_micros(1_000_000 / s.fps as u64);
            let el = now.saturating_duration_since(s.last_draw);
            if el < min {
                if !s.timer_armed {
                    s.timer_armed = true;
                    let kind = s.kind;
                    let _ = self.lh.insert_source(Timer::from_duration(min - el), move |_, _, app: &mut App| {
                        if let Some(s) = app.surf_mut(kind) {
                            s.timer_armed = false;
                            s.dirty = true;
                        }
                        app.flush();
                        TimeoutAction::Drop
                    });
                }
                return;
            }
        }
        let bw = (s.w as i32 * s.scale).max(1);
        let bh = (s.h as i32 * s.scale).max(1);
        let stride = bw * 4;
        let len = (stride * bh) as usize;
        if s.pool.is_none() {
            match SlotPool::new(len, shm) {
                Ok(p) => s.pool = Some(p),
                Err(_) => return,
            }
        }
        let pool = s.pool.as_mut().unwrap();
        let fmt = if abgr { wl_shm::Format::Abgr8888 } else { wl_shm::Format::Argb8888 };
        let Ok((buffer, canvas)) = pool.create_buffer(bw, bh, stride, fmt) else { return };
        let Some(pm) = PixmapMut::from_bytes(canvas, bw as u32, bh as u32) else { return };
        gfx.rebuilt = false;
        let first = s.draws == 0;
        let mut cv = Canvas::new(pm, s.scale as f32, gfx);
        cv.clear();
        let mut hits = Hits::new(s.hover, s.pressed);
        let ctx = Ctx { cfg, st, th: theme, now, sw, sh, seek: svc.seek_preview, edit: ui.edit, apps: &svc.apps.list, app_icons: &svc.app_icons };
        let out: Out = match s.kind {
            Kind::Wall(id) => ui::wall::draw(&mut cv, &ctx, ui, id),
            Kind::Bar => ui::bar::draw(&mut cv, &ctx, ui, &mut hits),
            Kind::Card => ui::card::draw(&mut cv, &ctx, ui, &mut hits),
            Kind::Launcher => ui::launcher::draw(&mut cv, &ctx, ui, &mut hits),
            Kind::Control => ui::control::draw(&mut cv, &ctx, ui, &mut hits),
            Kind::Power => ui::power::draw(&mut cv, &ctx, ui, &mut hits),
            Kind::Picker => ui::picker::draw(&mut cv, &ctx, ui, &mut hits),
            Kind::Settings => ui::settings::draw(&mut cv, &ctx, ui, &mut hits),
            Kind::Widget(w) => ui::widgets::draw(&mut cv, &ctx, ui, &mut hits, w),
            Kind::Viz => ui::widgets::draw_viz(&mut cv, &ctx, ui),
        };
        drop(cv);
        let gfx_rebuilt = gfx.rebuilt;
        if !abgr {
            swizzle(pool.canvas(&buffer).unwrap_or(&mut []));
        }
        s.hits = hits.list;
        s.dirty = false;
        s.anim = out.anim;
        s.last_draw = now;
        if out.done {
            s.closing = true;
            s.anim = false;
        }
        let wl = s.layer.wl_surface();
        // input region (only re-sent when it changes)
        let sig = match &out.input {
            None => 0,
            Some(v) => v.iter().fold(1469598103934665603u64, |h, r| {
                let mut h = h;
                for f in [r.0, r.1, r.2, r.3] {
                    h = (h ^ (f.round() as i64 as u64)).wrapping_mul(1099511628211);
                }
                h
            }),
        };
        if sig != s.input_sig {
            s.input_sig = sig;
            match &out.input {
                None => wl.set_input_region(None),
                Some(v) => {
                    if let Ok(r) = Region::new(compositor) {
                        for (x, y, w, h) in v {
                            r.add(*x as i32, *y as i32, w.ceil() as i32, h.ceil() as i32);
                        }
                        wl.set_input_region(Some(r.wl_region()));
                    }
                }
            }
        }
        match &out.damage {
            Some(rects) if !gfx_rebuilt && !first => {
                let sc = s.scale as f32;
                for (x, y, w, h) in rects {
                    wl.damage_buffer(
                        (x * sc).floor() as i32,
                        (y * sc).floor() as i32,
                        (w * sc).ceil() as i32 + 2,
                        (h * sc).ceil() as i32 + 2,
                    );
                }
            }
            _ => wl.damage_buffer(0, 0, bw, bh),
        }
        if s.anim {
            wl.frame(&qh, FrameCallbackData(wl.clone()));
            s.waiting_frame = true;
        }
        if buffer.attach_to(wl).is_ok() {
            s.layer.commit();
        }
        s.last_buf = Some(buffer);
        s.draws += 1;
        s.draw_us += now.elapsed().as_micros() as u64;
        if std::env::var_os("DSHELL_DEBUG").is_some() {
            eprintln!("draw {:?} anim={} t={:?}", s.kind, s.anim, self.started.elapsed());
        }
        s.pool_len = s.pool.as_ref().map(|p| p.len()).unwrap_or(0);
        if !s.anim {
            self.arm_trim(Duration::from_secs(3));
        }
    }

    /// A while after things went still: give back the pixel memory of surfaces that do not move.
    fn arm_trim(&mut self, after: Duration) {
        if self.trim_armed {
            return;
        }
        self.trim_armed = true;
        let _ = self.lh.insert_source(Timer::from_duration(after), |_, _, app: &mut App| {
            app.trim_armed = false;
            let now = Instant::now();
            let mut again = false;
            for s in &mut app.surfs {
                if s.pool.is_none() || s.anim || s.dirty {
                    continue;
                }
                if now.saturating_duration_since(s.last_draw) < Duration::from_secs(2) {
                    again = true;
                    continue;
                }
                // released by the compositor? then the memory can go
                let released = match (&s.last_buf, s.pool.as_mut()) {
                    (Some(b), Some(p)) => b.canvas(p).is_some(),
                    _ => true,
                };
                if released {
                    s.last_buf = None;
                    s.pool = None;
                } else {
                    again = true;
                }
            }
            app.gfx.trim_layers();
            // no panel open: the glyph and icon caches go too (they refill in microseconds)
            if !app.surfs.iter().any(|s| matches!(s.kind, Kind::Launcher | Kind::Control | Kind::Card | Kind::Power | Kind::Picker | Kind::Settings)) {
                app.gfx.trim();
            }
            if app.cfg.performance.trim_memory {
                // SAFETY: glibc call with no preconditions; returns freed heap pages to the system.
                #[cfg(target_env = "gnu")]
                unsafe {
                    libc::malloc_trim(0);
                }
            }
            if again {
                app.arm_trim(Duration::from_secs(3));
            }
            TimeoutAction::Drop
        });
    }

    // ------------------------------------------------------------------ input

    fn surf_index(&self, wl: &wl_surface::WlSurface) -> Option<usize> {
        self.surfs.iter().position(|s| s.layer.wl_surface() == wl)
    }

    /// Testing without a mouse: a click (or a wheel step) at surface coordinates of a surface.
    pub fn debug_click(&mut self, kind: Kind, x: f32, y: f32, wheel: f32) -> String {
        let Some(i) = self.surfs.iter().position(|s| s.kind == kind) else { return "no such surface".into() };
        let Some(h) = self.hit_at(i, x, y).cloned() else {
            if wheel != 0.0 {
                ui::scroll_surface(self, kind, wheel);
                self.flush();
                return "scrolled".into();
            }
            ui::press_empty(self, kind, 0x110, x, y);
            self.flush();
            return "nothing there".into();
        };
        let r = format!("{:?}", h.act);
        if wheel != 0.0 && h.scroll != Act::None {
            ui::scroll(self, &h.scroll, wheel);
        } else if wheel != 0.0 {
            ui::scroll_surface(self, kind, wheel);
        } else if h.kind == HitKind::Slider {
            ui::slide(self, &h.act, h.frac(x));
            ui::slide_end(self, &h.act);
        } else {
            ui::act(self, h.act.clone());
        }
        self.flush();
        r
    }

    fn hit_at(&self, i: usize, x: f32, y: f32) -> Option<&Hit> {
        self.surfs[i].hits.iter().rev().find(|h| h.contains(x, y))
    }

    fn set_hover(&mut self, i: usize, id: Option<u32>) {
        let s = &mut self.surfs[i];
        if s.hover != id {
            s.hover = id;
            s.dirty = true;
            let kind = s.kind;
            let act = id.and_then(|id| s.hits.iter().find(|h| h.id == id)).map(|h| h.act.clone());
            ui::hover(self, kind, act);
        }
    }

    fn pointer_event(&mut self, ev: &PointerEvent) {
        let Some(i) = self.surf_index(&ev.surface) else { return };
        let (x, y) = (ev.position.0 as f32, ev.position.1 as f32);
        let kind = self.surfs[i].kind;
        match &ev.kind {
            PointerEventKind::Enter { serial } => {
                self.ptr.surf = Some(ev.surface.clone());
                self.ptr.serial = *serial;
                if let Some(d) = &self.cursor_dev {
                    d.set_shape(*serial, CursorShape::Default);
                }
                self.ptr.x = x;
                self.ptr.y = y;
                let id = self.hit_at(i, x, y).map(|h| h.id);
                self.set_hover(i, id);
                ui::on_enter(self, kind, true);
            }
            PointerEventKind::Leave { .. } => {
                self.ptr.surf = None;
                self.set_hover(i, None);
                ui::on_enter(self, kind, false);
            }
            PointerEventKind::Motion { .. } => {
                self.ptr.x = x;
                self.ptr.y = y;
                if let Some((pk, pid, _)) = self.ptr.pressed
                    && pk == kind
                {
                    let hit = self.surfs[i].hits.iter().find(|h| h.id == pid).cloned();
                    if let Some(h) = hit
                        && h.kind == HitKind::Slider
                    {
                        let f = h.frac(x);
                        ui::slide(self, &h.act, f);
                    }
                    return;
                }
                if ui::drag_motion(self, kind, x, y) {
                    return;
                }
                let id = self.hit_at(i, x, y).map(|h| h.id);
                self.set_hover(i, id);
            }
            PointerEventKind::Press { button, serial, .. } => {
                self.ptr.serial = *serial;
                let hit = self.hit_at(i, x, y).cloned();
                if let Some(h) = hit {
                    self.ptr.pressed = Some((kind, h.id, *button));
                    self.surfs[i].pressed = Some(h.id);
                    self.surfs[i].dirty = true;
                    if h.kind == HitKind::Slider && *button == 0x110 {
                        ui::slide(self, &h.act, h.frac(x));
                    }
                } else {
                    ui::press_empty(self, kind, *button, x, y);
                }
            }
            PointerEventKind::Release { button, .. } => {
                if ui::drag_end(self, kind) {
                    return;
                }
                let pressed = self.ptr.pressed.take();
                if let Some(i) = self.surf_index(&ev.surface) {
                    self.surfs[i].pressed = None;
                    self.surfs[i].dirty = true;
                    let hit = self.hit_at(i, x, y).cloned();
                    if let (Some((pk, pid, pb)), Some(h)) = (pressed, hit)
                        && pk == kind
                        && pid == h.id
                        && pb == *button
                    {
                        match (*button, h.kind) {
                            (0x110, HitKind::Click) => ui::act(self, h.act.clone()),
                            (0x110, HitKind::Slider) => ui::slide_end(self, &h.act),
                            (0x111, _) if h.right != Act::None => ui::act(self, h.right.clone()),
                            (0x112, _) => {}
                            _ => {}
                        }
                    }
                }
            }
            PointerEventKind::Axis { vertical, .. } => {
                let mut d = if vertical.value120 != 0 {
                    vertical.value120 as f32 / 120.0
                } else if vertical.discrete != 0 {
                    vertical.discrete as f32
                } else {
                    vertical.absolute as f32 / 50.0
                };
                if d.abs() < 0.01 {
                    return;
                }
                d = d.clamp(-3.0, 3.0);
                let hit = self.hit_at(i, x, y).cloned();
                if let Some(h) = hit
                    && h.scroll != Act::None
                {
                    ui::scroll(self, &h.scroll, d);
                } else {
                    ui::scroll_surface(self, kind, d);
                }
            }
        }
        self.flush();
    }
}

/// tiny-skia draws RGBA; wl_shm's always-there format is BGRA in memory.
fn swizzle(buf: &mut [u8]) {
    for px in buf.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
}

// ---------------------------------------------------------------------- handlers

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, f: i32) {
        if std::env::var_os("DSHELL_DEBUG").is_some() {
            eprintln!("scale {f}");
        }
        if let Some(i) = self.surf_index(surface) {
            let s = &mut self.surfs[i];
            if s.scale != f.max(1) {
                s.scale = f.max(1);
                s.layer.wl_surface().set_buffer_scale(s.scale);
                s.pool = None;
                s.last_buf = None;
                s.dirty = true;
            }
        }
        self.flush();
    }
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, _: u32) {
        if let Some(i) = self.surf_index(surface) {
            let s = &mut self.surfs[i];
            s.waiting_frame = false;
            if s.anim {
                s.dirty = true;
            }
        }
        self.flush();
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        crate::shell::outputs_changed(self);
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        crate::shell::outputs_changed(self);
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, o: wl_output::WlOutput) {
        let gone: Vec<Kind> = self.surfs.iter().filter(|s| s.output.as_ref() == Some(&o)).map(|s| s.kind).collect();
        for k in gone {
            self.destroy(k);
        }
        if self.svc.main_output.as_ref() == Some(&o) {
            self.svc.main_output = None;
        }
        crate::shell::outputs_changed(self);
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if let Some(i) = self.surf_index(layer.wl_surface()) {
            let k = self.surfs[i].kind;
            self.destroy(k);
        }
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface, c: LayerSurfaceConfigure, _: u32) {
        if let Some(i) = self.surf_index(layer.wl_surface()) {
            let s = &mut self.surfs[i];
            let (w, h) = c.new_size;
            if std::env::var_os("DSHELL_DEBUG").is_some() {
                eprintln!("configure {:?} {:?}", s.kind, c.new_size);
            }
            // compositors re-send the same configure whenever any layer surface changes: only a new
            // size (or the first configure) needs a new buffer
            let resized = w > 0 && h > 0 && (w != s.w || h != s.h);
            if resized {
                s.w = w;
                s.h = h;
                s.pool = None;
                s.last_buf = None;
            }
            if resized || !s.configured || s.draws == 0 {
                s.dirty = true;
                s.waiting_frame = false;
            }
            s.configured = true;
        }
        self.flush();
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Keyboard && self.keyboard.is_none() {
            let lh = self.lh.clone();
            if let Ok(k) = self.seat.get_keyboard_with_repeat(
                qh,
                &seat,
                None,
                lh,
                Box::new(|app: &mut App, _: &wl_keyboard::WlKeyboard, ev: KeyEvent| {
                    ui::key(app, &ev);
                    app.flush();
                }),
            ) {
                self.keyboard = Some(k);
            }
        }
        if cap == Capability::Pointer && self.pointer.is_none() {
            if let Ok(p) = self.seat.get_pointer(qh, &seat) {
                if let Some(m) = &self.cursor_mgr {
                    self.cursor_dev = Some(m.get_shape_device(&p, qh));
                }
                self.pointer = Some(p);
            }
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Keyboard
            && let Some(k) = self.keyboard.take()
        {
            k.release();
        }
        if cap == Capability::Pointer
            && let Some(p) = self.pointer.take()
        {
            p.release();
            self.cursor_dev = None;
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        if let Some(i) = self.surf_index(surface) {
            self.kb_focus = Some(self.surfs[i].kind);
        }
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, surface: &wl_surface::WlSurface, _: u32) {
        if let Some(i) = self.surf_index(surface) {
            let k = self.surfs[i].kind;
            if self.kb_focus == Some(k) {
                self.kb_focus = None;
            }
            ui::focus_lost(self, k);
            self.flush();
        }
    }
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, ev: KeyEvent) {
        ui::key(self, &ev);
        self.flush();
    }
    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, ev: KeyEvent) {
        ui::key(self, &ev);
        self.flush();
    }
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        m: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.mods = m;
    }
}

impl PointerHandler for App {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for e in events {
            self.pointer_event(e);
        }
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_registry!(App);
smithay_client_toolkit::delegate_dispatch2!(App);

// ---------------------------------------------------------------------- run

pub fn run() -> Result<(), String> {
    // Memory: big buffers (decoded pictures) come straight from mmap and go straight back; worker threads
    // share the main heap instead of keeping their own arenas around.
    #[cfg(target_env = "gnu")]
    // SAFETY: glibc tuning calls made before any other thread exists.
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 1);
        libc::mallopt(libc::M_MMAP_THRESHOLD, 128 * 1024);
        libc::mallopt(libc::M_TRIM_THRESHOLD, 256 * 1024);
    }
    let conn = Connection::connect_to_env().map_err(|e| format!("no Wayland session: {e}"))?;
    let (globals, queue) = registry_queue_init(&conn).map_err(|e| e.to_string())?;
    let qh = queue.handle();
    let mut ev: EventLoop<'static, App> = EventLoop::try_new().map_err(|e| e.to_string())?;
    let lh = ev.handle();
    WaylandSource::new(conn.clone(), queue).insert(lh.clone()).map_err(|e| e.to_string())?;

    let compositor = CompositorState::bind(&globals, &qh).map_err(|_| "wl_compositor missing")?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(|_| "this compositor has no layer shell")?;
    let shm = Shm::bind(&globals, &qh).map_err(|_| "wl_shm missing")?;
    let cursor_mgr = CursorShapeManager::bind(&globals, &qh).ok();

    let (cfg, cfg_err) = crate::config::load();
    let saved = crate::config::load_saved();
    let st = State::default();
    let theme = make_theme(&cfg, &st);
    let mut app = App {
        qh: qh.clone(),
        lh: lh.clone(),
        registry: RegistryState::new(&globals),
        seat: SeatState::new(&globals, &qh),
        outputs: OutputState::new(&globals, &qh),
        compositor,
        layer_shell,
        abgr: false,
        shm,
        cursor_mgr,
        cursor_dev: None,
        pointer: None,
        keyboard: None,
        surfs: Vec::new(),
        gfx: Gfx::new(),
        cfg,
        cfg_err,
        saved,
        st,
        ui: Ui::new(),
        theme,
        svc: crate::svc::Services::default(),
        ptr: Ptr::default(),
        kb_focus: None,
        mods: Modifiers::default(),
        exit: false,
        started: Instant::now(),
        trim_armed: false,
    };
    // the first roundtrips: outputs, seats and the shm formats
    ev.dispatch(Some(Duration::from_millis(50)), &mut app).map_err(|e| e.to_string())?;
    let _ = conn.roundtrip();
    ev.dispatch(Some(Duration::from_millis(20)), &mut app).map_err(|e| e.to_string())?;
    app.abgr = app.shm.formats().contains(&wl_shm::Format::Abgr8888) && std::env::var_os("DSHELL_ARGB").is_none();

    crate::shell::start(&mut app);
    crate::ipc::serve(&mut app);
    app.flush();

    // A bug in one panel must not take the desktop down: recover and keep going.
    let mut crashes: Vec<Instant> = Vec::new();
    while !app.exit {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
            ev.dispatch(None, &mut app).map_err(|e| e.to_string())?;
            // D-Bus messages that arrived while we waited for a reply sit in libdbus's queue
            crate::svc::bus::drain(&mut app);
            app.flush();
            Ok(())
        }));
        match r {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                crashes.retain(|t| t.elapsed() < Duration::from_secs(60));
                crashes.push(Instant::now());
                if crashes.len() > 20 {
                    return Err("too many errors in a minute, giving up".into());
                }
                // close the panels (one of them may be what failed) and redraw the rest
                let panels: Vec<Kind> = app
                    .surfs
                    .iter()
                    .map(|s| s.kind)
                    .filter(|k| matches!(k, Kind::Launcher | Kind::Control | Kind::Card | Kind::Power | Kind::Picker | Kind::Settings))
                    .collect();
                for k in panels {
                    app.destroy(k);
                }
                for s in &mut app.surfs {
                    s.waiting_frame = false;
                    s.dirty = true;
                }
            }
        }
    }
    Ok(())
}

