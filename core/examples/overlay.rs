//! Renders a labelling view of one real photo: a 10% grid with numbered lines,
//! the ground-truth quad (green) and optionally the detected quad (red).
//!
//!   cargo run --release -p easyscan-core --example overlay -- <n> [det]
//!
//! Output: `testdata/photos/_out/<n>.jpg` (gitignored with the photos).

mod support;

use image::{DynamicImage, Rgb, RgbImage, imageops::FilterType};

const DIGITS: [[u8; 5]; 10] = [
    [7, 5, 5, 5, 7],
    [2, 6, 2, 2, 7],
    [7, 1, 7, 4, 7],
    [7, 1, 7, 1, 7],
    [5, 5, 7, 1, 1],
    [7, 4, 7, 1, 7],
    [7, 4, 7, 5, 7],
    [7, 1, 1, 1, 1],
    [7, 5, 7, 5, 7],
    [7, 5, 7, 1, 7],
];

fn digit(img: &mut RgbImage, d: usize, x: i32, y: i32, col: Rgb<u8>) {
    for (r, bits) in DIGITS[d].iter().enumerate() {
        for c in 0..3 {
            if bits >> (2 - c) & 1 == 1 {
                for dy in 0..4 {
                    for dx in 0..4 {
                        let (px, py) = (x + c * 4 + dx, y + r as i32 * 4 + dy);
                        if px >= 0
                            && py >= 0
                            && (px as u32) < img.width()
                            && (py as u32) < img.height()
                        {
                            img.put_pixel(px as u32, py as u32, col);
                        }
                    }
                }
            }
        }
    }
}

fn line(img: &mut RgbImage, a: (f32, f32), b: (f32, f32), col: Rgb<u8>, thick: i32) {
    let n = ((b.0 - a.0).abs().max((b.1 - a.1).abs()) as i32).max(1);
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        for dy in 0..thick {
            for dx in 0..thick {
                let (px, py) = (x as i32 + dx, y as i32 + dy);
                if px >= 0 && py >= 0 && (px as u32) < img.width() && (py as u32) < img.height() {
                    img.put_pixel(px as u32, py as u32, col);
                }
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let n: usize = args[0].parse().expect("photo index");
    let with_det = args.iter().any(|a| a == "det");
    let path = support::photos()
        .into_iter()
        .find(|(i, _)| *i == n)
        .expect("photo not found")
        .1;
    let img = support::load_oriented(&path);
    if args.iter().any(|a| a == "zoom") {
        return zoom(n, &img, with_det);
    }
    let (ow, oh) = (img.width(), img.height());
    let scale = 960.0 / ow.max(oh) as f32;
    let (w, h) = ((ow as f32 * scale) as u32, (oh as f32 * scale) as u32);
    let mut view = img.resize_exact(w, h, FilterType::Triangle).to_rgb8();

    // Grid: faint lines, a numbered tenth at the top and the left.
    let grid = Rgb([255, 255, 0]);
    for t in 1..10 {
        let f = t as f32 / 10.0;
        line(
            &mut view,
            (f * w as f32, 0.0),
            (f * w as f32, h as f32),
            grid,
            1,
        );
        line(
            &mut view,
            (0.0, f * h as f32),
            (w as f32, f * h as f32),
            grid,
            1,
        );
        digit(
            &mut view,
            t,
            (f * w as f32) as i32 + 2,
            2,
            Rgb([255, 0, 255]),
        );
        digit(
            &mut view,
            t,
            2,
            (f * h as f32) as i32 + 2,
            Rgb([255, 0, 255]),
        );
    }
    let draw = |view: &mut RgbImage, q: &[(f32, f32); 4], col: Rgb<u8>| {
        for i in 0..4 {
            let (a, b) = (q[i], q[(i + 1) % 4]);
            line(
                view,
                (a.0 * w as f32, a.1 * h as f32),
                (b.0 * w as f32, b.1 * h as f32),
                col,
                2,
            );
        }
    };
    if let Some(gt) = support::labels().get(&n) {
        draw(&mut view, gt, Rgb([0, 255, 0]));
    }
    if with_det {
        let d = easyscan_core::detect::detect_page(&img);
        let q = d.quad.0.map(|p| (p.x / ow as f32, p.y / oh as f32));
        println!("found={} conf={:.2} {q:?}", d.found, d.confidence);
        draw(&mut view, &q, Rgb([255, 0, 0]));
    }
    let out = support::photos_dir().join("_out").join(format!("{n}.jpg"));
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    DynamicImage::ImageRgb8(view).save(&out).unwrap();
    println!("{}", out.display());
}

/// `zoom` mode: four 360px crops (one per labelled corner, TL TR / BL BR) cut
/// from the full-resolution photo, centred on the label (green cross) with the
/// detected corner (red cross) and a grid every 30 crop pixels, to refine labels.
pub fn zoom(n: usize, img: &DynamicImage, with_det: bool) {
    let (ow, oh) = (img.width() as f32, img.height() as f32);
    let rgb = img.to_rgb8();
    let gt = support::labels().get(&n).copied().expect("label first");
    let det = easyscan_core::detect::detect_page(img).quad.0;
    const SIDE: f32 = 360.0; // full-res pixels per crop (1:1)
    const OUT: u32 = 360;
    let k = OUT as f32 / SIDE;
    let mut sheet = RgbImage::new(OUT * 2 + 4, OUT * 2 + 4);
    for (ci, order) in [0usize, 1, 3, 2].into_iter().enumerate() {
        let (cx, cy) = (gt[order].0 * ow, gt[order].1 * oh);
        let (x0, y0) = (cx - SIDE / 2.0, cy - SIDE / 2.0);
        let (ox, oy) = ((ci % 2) as u32 * (OUT + 4), (ci / 2) as u32 * (OUT + 4));
        for y in 0..OUT {
            for x in 0..OUT {
                let (sx, sy) = (x0 + x as f32 / k, y0 + y as f32 / k);
                let px = if sx >= 0.0 && sy >= 0.0 && sx < ow && sy < oh {
                    *rgb.get_pixel(sx as u32, sy as u32)
                } else {
                    Rgb([0, 0, 0])
                };
                sheet.put_pixel(ox + x, oy + y, px);
            }
        }
        let mut view = RgbImage::new(OUT, OUT);
        for y in 0..OUT {
            for x in 0..OUT {
                view.put_pixel(x, y, *sheet.get_pixel(ox + x, oy + y));
            }
        }
        for g in (30..OUT as i32).step_by(30) {
            line(
                &mut view,
                (g as f32, 0.0),
                (g as f32, OUT as f32),
                Rgb([255, 255, 0]),
                1,
            );
            line(
                &mut view,
                (0.0, g as f32),
                (OUT as f32, g as f32),
                Rgb([255, 255, 0]),
                1,
            );
        }
        let c = OUT as f32 / 2.0;
        line(&mut view, (c - 14.0, c), (c + 14.0, c), Rgb([0, 255, 0]), 2);
        line(&mut view, (c, c - 14.0), (c, c + 14.0), Rgb([0, 255, 0]), 2);
        if with_det {
            let (dx, dy) = ((det[order].x - x0) * k, (det[order].y - y0) * k);
            line(
                &mut view,
                (dx - 12.0, dy - 12.0),
                (dx + 12.0, dy + 12.0),
                Rgb([255, 0, 0]),
                2,
            );
            line(
                &mut view,
                (dx - 12.0, dy + 12.0),
                (dx + 12.0, dy - 12.0),
                Rgb([255, 0, 0]),
                2,
            );
        }
        for y in 0..OUT {
            for x in 0..OUT {
                sheet.put_pixel(ox + x, oy + y, *view.get_pixel(x, y));
            }
        }
    }
    let out = support::photos_dir()
        .join("_out")
        .join(format!("{n}_z.jpg"));
    DynamicImage::ImageRgb8(sheet).save(&out).unwrap();
    println!("{}", out.display());
}
