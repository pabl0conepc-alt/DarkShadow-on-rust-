//! Times the drawing primitives: `cargo run --release --example drawbench`
#[path = "../src/color.rs"]
mod color;
#[path = "../src/icons_data.rs"]
mod icons_data;
#[path = "../src/text.rs"]
mod text;
mod ui {
    #[derive(Clone, Debug)]
    pub struct Hit;
    pub struct Hits {
        pub list: Vec<Hit>,
        pub base: u32,
        pub hover: Option<u32>,
        pub pressed: Option<u32>,
        pub ox: f32,
        pub oy: f32,
    }
    impl Hits {
        pub fn new(hover: Option<u32>, pressed: Option<u32>) -> Self {
            Hits {
                list: vec![],
                base: 0,
                hover,
                pressed,
                ox: 0.0,
                oy: 0.0,
            }
        }
    }
}
#[path = "../src/gfx.rs"]
mod gfx;
#[path = "../src/raster.rs"]
mod raster;
use std::time::Instant;

fn main() {
    let mut g = gfx::Gfx::new();
    let mut pm = tiny_skia::Pixmap::new(356, 416).unwrap();
    let mut data = vec![255u8; 300 * 300 * 4];
    for (i, b) in data.iter_mut().enumerate() {
        *b = (i * 7 % 251) as u8;
    }
    let img = gfx::Image::from_rgba(300, 300, data).unwrap();
    let c = color::rgba(1.0, 1.0, 1.0, 1.0);
    let n = 300;
    macro_rules! bench {
        ($name:expr, $body:expr) => {{
            let t = Instant::now();
            for i in 0..n {
                let mut cv = gfx::Canvas::new(pm.as_mut(), 1.0, &mut g);
                let _ = i;
                $body(&mut cv, i as f32);
            }
            println!(
                "{:<28} {:>7.1} us",
                $name,
                t.elapsed().as_secs_f64() * 1e6 / n as f64
            );
        }};
    }
    bench!("clear", |cv: &mut gfx::Canvas, _| cv.clear());
    bench!("memcpy 590KB", |cv: &mut gfx::Canvas, _| {
        let d = cv.pm.data_mut();
        d.copy_from_slice(&vec![0u8; d.len()]);
    });
    bench!("image_shape rot", |cv: &mut gfx::Canvas, i: f32| cv
        .image_shape(
            &img,
            gfx::Shape::Cookie(9),
            178.0,
            136.0,
            76.0,
            i * 0.01
        ));
    bench!("image_shape circle", |cv: &mut gfx::Canvas, _| cv
        .image_shape(&img, gfx::Shape::Circle, 178.0, 136.0, 76.0, 0.0));
    bench!("shape fill cookie", |cv: &mut gfx::Canvas, i: f32| cv
        .shape(gfx::Shape::Cookie(9), 178.0, 136.0, 76.0, i * 0.01, c));
    bench!("wavy_arc full", |cv: &mut gfx::Canvas, i: f32| cv
        .wavy_arc(178.0, 136.0, 88.0, 2.6, 14.0, i, 0.0, 6.28, 3.0, c));
    bench!("wavy_line 300px", |cv: &mut gfx::Canvas, i: f32| cv
        .wavy_line(20.0, 320.0, 300.0, 3.0, 22.0, i, 3.0, c));
    bench!("shadow card", |cv: &mut gfx::Canvas, _| cv
        .shadow(18.0, 18.0, 320.0, 380.0, 30.0, 14.0, 0.5));
    bench!("rrect card", |cv: &mut gfx::Canvas, _| cv.rrect(
        18.0,
        18.0,
        320.0,
        380.0,
        30.0,
        color::rgba(0.1, 0.1, 0.1, 0.5)
    ));
    bench!("text 30 chars", |cv: &mut gfx::Canvas, _| {
        cv.text(
            20.0,
            200.0,
            "SON6FORMYD34TH - Slowed – STAKI",
            text::Face::Semi,
            14.0,
            c,
            gfx::Align::Left,
        );
    });
    bench!("fill_rect 320x380", |cv: &mut gfx::Canvas, _| {
        let mut p = tiny_skia::Paint::default();
        p.set_color_rgba8(20, 20, 20, 128);
        cv.pm.fill_rect(
            tiny_skia::Rect::from_xywh(18.0, 18.0, 320.0, 380.0).unwrap(),
            &p,
            tiny_skia::Transform::identity(),
            None,
        );
    });
    bench!("fill_path rect noAA", |cv: &mut gfx::Canvas, _| {
        let mut p = tiny_skia::Paint::default();
        p.set_color_rgba8(20, 20, 20, 128);
        p.anti_alias = false;
        let path = gfx::rrect_path(18.0, 18.0, 320.0, 380.0, 30.0).unwrap();
        cv.pm.fill_path(
            &path,
            &p,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    });
    let small = {
        let mut d = vec![0u8; 152 * 152 * 4];
        for (i, b) in d.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        gfx::Image::from_rgba(152, 152, d).unwrap()
    };
    bench!("pattern nearest 152", |cv: &mut gfx::Canvas, _| {
        let pat = tiny_skia::Pattern::new(
            small.pm.as_ref(),
            tiny_skia::SpreadMode::Pad,
            tiny_skia::FilterQuality::Nearest,
            1.0,
            tiny_skia::Transform::from_translate(102.0, 60.0),
        );
        let p = tiny_skia::Paint {
            shader: pat,
            anti_alias: true,
            ..Default::default()
        };
        let path = gfx::shape_path(gfx::Shape::Cookie(9), 178.0, 136.0, 76.0, 0.0).unwrap();
        cv.pm.fill_path(
            &path,
            &p,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    });
    bench!("draw_pixmap 152", |cv: &mut gfx::Canvas, _| {
        cv.pm.draw_pixmap(
            102,
            60,
            small.pm.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            tiny_skia::Transform::identity(),
            None,
        );
    });
    bench!("raster rrect opaque", |cv: &mut gfx::Canvas, _| {
        let (w, h) = (cv.pm.width() as i32, cv.pm.height() as i32);
        let mut t = raster::Target {
            data: cv.pm.data_mut(),
            w,
            h,
        };
        t.rrect(18.0, 18.0, 320.0, 380.0, 30.0, [20, 20, 20, 255]);
    });
    bench!("raster rrect alpha", |cv: &mut gfx::Canvas, _| {
        let (w, h) = (cv.pm.width() as i32, cv.pm.height() as i32);
        let mut t = raster::Target {
            data: cv.pm.data_mut(),
            w,
            h,
        };
        t.rrect(18.0, 18.0, 320.0, 380.0, 30.0, [10, 10, 10, 128]);
    });
    bench!("icon 24", |cv: &mut gfx::Canvas, _| cv
        .icon("pause", 100.0, 100.0, 24.0, c));
}
