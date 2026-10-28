//! Renders a labelling view of one real photo: a 10% grid with numbered lines,
//! the ground-truth quad (green) and optionally the detected quad (red).
//!
//!   cargo run --release -p easyscan-core --example overlay -- <n> [det]
//!
//! Output: `testdata/photos/_out/<n>.jpg` (gitignored with the photos).

mod support;

use image::{DynamicImage, Rgb, RgbImage, imageops::FilterType};

const DIGITS: [[u8; 5]; 10] = [
    [7, 5, 5, 5, 7], [2, 6, 2, 2, 7], [7, 1, 7, 4, 7], [7, 1, 7, 1, 7], [5, 5, 7, 1, 1],
    [7, 4, 7, 1, 7], [7, 4, 7, 5, 7], [7, 1, 1, 1, 1], [7, 5, 7, 5, 7], [7, 5, 7, 1, 7],
];

fn digit(img: &mut RgbImage, d: usize, x: i32, y: i32, col: Rgb<u8>) {
    for (r, bits) in DIGITS[d].iter().enumerate() {
        for c in 0..3 {
            if bits >> (2 - c) & 1 == 1 {
                for dy in 0..4 {
                    for dx in 0..4 {
                        let (px, py) = (x + c * 4 + dx, y + r as i32 * 4 + dy);
                        if px >= 0 && py >= 0 && (px as u32) < img.width() && (py as u32) < img.height() {
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
    let with_det = args.get(1).is_some_and(|a| a == "det");
    let path = support::photos().into_iter().find(|(i, _)| *i == n).expect("photo not found").1;
    let img = support::load_oriented(&path);
    let (ow, oh) = (img.width(), img.height());
    let scale = 960.0 / ow.max(oh) as f32;
    let (w, h) = ((ow as f32 * scale) as u32, (oh as f32 * scale) as u32);
    let mut view = img.resize_exact(w, h, FilterType::Triangle).to_rgb8();

    // Grid: faint lines, a numbered tenth at the top and the left.
    let grid = Rgb([255, 255, 0]);
    for t in 1..10 {
        let f = t as f32 / 10.0;
        line(&mut view, (f * w as f32, 0.0), (f * w as f32, h as f32), grid, 1);
        line(&mut view, (0.0, f * h as f32), (w as f32, f * h as f32), grid, 1);
        digit(&mut view, t, (f * w as f32) as i32 + 2, 2, Rgb([255, 0, 255]));
        digit(&mut view, t, 2, (f * h as f32) as i32 + 2, Rgb([255, 0, 255]));
    }
    let draw = |view: &mut RgbImage, q: &[(f32, f32); 4], col: Rgb<u8>| {
        for i in 0..4 {
            let (a, b) = (q[i], q[(i + 1) % 4]);
            line(view, (a.0 * w as f32, a.1 * h as f32), (b.0 * w as f32, b.1 * h as f32), col, 2);
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
