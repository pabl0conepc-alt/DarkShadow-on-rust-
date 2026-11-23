//! Pictures, decoded on a worker thread and scaled down right away so only small copies stay in memory.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::app::App;
use crate::gfx::Image;
use crate::state::topic;

fn decode_file(path: &Path, max: u32) -> Option<image::RgbaImage> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if ext == "svg" || ext == "svgz" {
        return render_svg(path, max);
    }
    let img = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?;
    let img = if img.width() > max || img.height() > max { img.thumbnail(max, max) } else { img };
    Some(img.to_rgba8())
}

fn render_svg(path: &Path, size: u32) -> Option<image::RgbaImage> {
    let data = std::fs::read(path).ok()?;
    let tree = resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default()).ok()?;
    let ts = tree.size();
    let s = size as f32 / ts.width().max(ts.height()).max(1.0);
    let (w, h) = (((ts.width() * s).ceil() as u32).max(1), ((ts.height() * s).ceil() as u32).max(1));
    let mut pm = tiny_skia::Pixmap::new(w, h)?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(s, s), &mut pm.as_mut());
    // back to straight alpha for the common path
    let mut data = pm.take();
    for px in data.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            px[0] = ((px[0] as u32 * 255 + a / 2) / a).min(255) as u8;
            px[1] = ((px[1] as u32 * 255 + a / 2) / a).min(255) as u8;
            px[2] = ((px[2] as u32 * 255 + a / 2) / a).min(255) as u8;
        }
    }
    image::RgbaImage::from_raw(w, h, data)
}

fn to_image(img: image::RgbaImage) -> Option<Image> {
    let (w, h) = (img.width(), img.height());
    Image::from_rgba(w, h, img.into_raw())
}

/// A square-ish picture (avatar, icon) at most `size` px.
pub fn load_square(app: &mut App, path: String, size: u32, done: impl FnOnce(&mut App, Option<Rc<Image>>) + 'static) {
    crate::proc::background(
        app,
        move || decode_file(Path::new(&path), size).map(|i| (i.width(), i.height(), i.into_raw())),
        move |app, r| {
            let img = r.and_then(|(w, h, d)| Image::from_rgba(w, h, d)).map(Rc::new);
            done(app, img);
        },
    );
}

fn art_cache(url: &str) -> PathBuf {
    let mut h: u64 = 1469598103934665603;
    for b in url.bytes() {
        h = (h ^ b as u64).wrapping_mul(1099511628211);
    }
    crate::config::cache_dir().join("art").join(format!("{h:016x}"))
}

/// Cover art: file:// paths directly, http(s) through curl into the cache (no TLS stack in the shell).
pub fn load_art(app: &mut App, url: String) {
    let want = url.clone();
    let finish = move |app: &mut App, img: Option<Rc<Image>>| {
        if app.st.media.art_url == want {
            app.st.media.art = img;
            app.mark(topic::MEDIA);
        }
    };
    if let Some(p) = url.strip_prefix("file://") {
        let p = percent_decode(p);
        load_square(app, p, 256, finish);
    } else if url.starts_with("http://") || url.starts_with("https://") {
        let dest = art_cache(&url);
        if dest.is_file() {
            load_square(app, dest.to_string_lossy().into_owned(), 256, finish);
            return;
        }
        let _ = std::fs::create_dir_all(dest.parent().unwrap());
        let d = dest.to_string_lossy().into_owned();
        let d2 = d.clone();
        crate::proc::capture(app, &["curl", "-fsSL", "-m", "10", "-o", &d2, &url], move |app, _| {
            if Path::new(&d).is_file() {
                load_square(app, d, 256, finish);
            }
        });
        prune_cache();
    }
}

fn prune_cache() {
    let dir = crate::config::cache_dir().join("art");
    if let Ok(rd) = std::fs::read_dir(&dir) {
        let mut v: Vec<(std::time::SystemTime, PathBuf)> =
            rd.flatten().filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path()))).collect();
        if v.len() > 60 {
            v.sort();
            for (_, p) in v.iter().take(v.len() - 60) {
                let _ = std::fs::remove_file(p);
            }
        }
    }
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A notification's picture: a path, a file:// URL or an icon name.
pub fn load_notif_icon(app: &mut App, id: u32, src: String) {
    let path = if let Some(p) = src.strip_prefix("file://") {
        Some(PathBuf::from(percent_decode(p)))
    } else if src.starts_with('/') {
        Some(PathBuf::from(&src))
    } else {
        let theme = app.cfg.apps.icon_theme.clone();
        app.svc.apps.find_icon(&src, &theme)
    };
    let Some(path) = path else { return };
    load_square(app, path.to_string_lossy().into_owned(), 96, move |app, img| {
        if let Some(n) = app.st.notifs.list.iter_mut().find(|n| n.id == id) {
            n.image = img;
            app.mark(topic::NOTIF);
        }
    });
}

/// An app icon for the launcher.
pub fn load_app_icon(app: &mut App, name: String) {
    if app.svc.app_icons_loading.contains(&name) || app.svc.app_icons.contains_key(&name) {
        return;
    }
    let theme = app.cfg.apps.icon_theme.clone();
    let Some(path) = app.svc.apps.find_icon(&name, &theme) else {
        app.svc.app_icons.insert(name, None);
        return;
    };
    app.svc.app_icons_loading.insert(name.clone());
    let key = name.clone();
    load_square(app, path.to_string_lossy().into_owned(), 96, move |app, img| {
        app.svc.app_icons_loading.remove(&key);
        if app.svc.app_icons.len() > 200 {
            app.svc.app_icons.clear();
        }
        app.svc.app_icons.insert(key, img);
        app.mark(topic::APPS);
    });
}

/// The wallpaper, scaled to cover `w` x `h`, plus its brightness grid and accent color.
pub struct WallPixels {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    pub lum: Vec<f32>,
    pub accent: crate::color::Rgba,
}

pub fn decode_wallpaper(path: &Path, w: u32, h: u32) -> Option<WallPixels> {
    let img = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?;
    let img = cover(img, w, h);
    drop_peak();
    let small = image::imageops::thumbnail(&img, 64, 36);
    let mut lum = Vec::with_capacity(64 * 36);
    let mut px = Vec::with_capacity(64 * 36);
    for p in small.pixels() {
        let c = crate::color::rgba(p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0, 1.0);
        lum.push(c.luma());
        px.push([p[0], p[1], p[2]]);
    }
    let accent = crate::theme::accent_from_pixels(&px);
    // RGBA for the buffer (opaque)
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for p in img.pixels() {
        rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
    }
    Some(WallPixels { w, h, rgba, lum, accent })
}

/// Scale a picture to cover w x h with little memory: crop to the aspect first, then an integer
/// area-average downscale (no float intermediate buffers); upscaling only happens for small pictures.
fn cover(img: image::DynamicImage, w: u32, h: u32) -> image::RgbImage {
    let src = img.into_rgb8();
    let (sw, sh) = (src.width().max(1), src.height().max(1));
    let k = (w as f32 / sw as f32).max(h as f32 / sh as f32);
    let (cw, ch) = (((w as f32 / k).round() as u32).clamp(1, sw), ((h as f32 / k).round() as u32).clamp(1, sh));
    let (cx, cy) = ((sw - cw) / 2, (sh - ch) / 2);
    let view = image::imageops::crop_imm(&src, cx, cy, cw, ch);
    if cw >= w && ch >= h {
        image::imageops::thumbnail(&*view, w, h)
    } else {
        let part = view.to_image();
        image::imageops::resize(&part, w, h, image::imageops::FilterType::Triangle)
    }
}

fn drop_peak() {
    // SAFETY: glibc call without preconditions; gives the decoder's freed pages back right away.
    #[cfg(target_env = "gnu")]
    unsafe {
        libc::malloc_trim(0);
    }
}

/// A wallpaper thumbnail for the picker.
pub fn load_thumb(app: &mut App, idx: usize, path: PathBuf) {
    // one at a time: a 4K JPEG needs ~30 MB while it decodes, so they are not decoded side by side
    if !app.ui.picker.loading.is_empty() || app.ui.picker.thumbs.contains_key(&idx) {
        return;
    }
    app.ui.picker.loading.insert(idx);
    crate::proc::background(
        app,
        move || {
            let img = image::ImageReader::open(&path).ok()?.with_guessed_format().ok()?.decode().ok()?;
            let t = img.resize_to_fill(320, 200, image::imageops::FilterType::Triangle).to_rgba8();
            Some((t.width(), t.height(), t.into_raw()))
        },
        move |app, r| {
            app.ui.picker.loading.remove(&idx);
            if let Some(img) = r.and_then(|(w, h, d)| to_image(image::RgbaImage::from_raw(w, h, d)?)) {
                app.ui.picker.thumbs.insert(idx, Rc::new(img));
                app.mark_kind(crate::ui::Kind::Picker);
            }
        },
    );
}
