//! Page detection and perspective correction.
//!
//! `detect_page` only *suggests* corners: when unsure it returns the full-image
//! corners with `found = false` and never errors. `warp` flattens a quad into a
//! rectangle at the source resolution.
//!
//! Detection works on a downscaled grayscale copy: blur, Otsu threshold, take
//! the largest bright connected blob, and reduce its convex hull to four
//! corners. A candidate is rejected if it is tiny, nearly the whole frame, a
//! poor fit for its blob, or badly skewed.

use image::{DynamicImage, GrayImage, Rgb, RgbImage};

/// Long side of the working copy used for detection.
const DETECT_SIZE: u32 = 640;
/// Smallest accepted page, as a fraction of the image area.
const MIN_AREA_FRAC: f32 = 0.10;
/// Largest accepted page; above this the "page" is just the whole frame.
const MAX_AREA_FRAC: f32 = 0.97;
/// Minimum blob area / quad area for the quad to be a believable fit.
const MIN_FILL: f32 = 0.80;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// Four corners ordered top-left, top-right, bottom-right, bottom-left, in
/// source-image pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad(pub [Point; 4]);

#[derive(Debug, Clone, Copy)]
pub struct Detection {
    pub quad: Quad,
    /// False when nothing page-like was found and `quad` is the full image.
    pub found: bool,
    /// 0..1 fit of the quad to the detected blob; 0 when not found.
    pub confidence: f32,
}

impl Quad {
    pub fn full(w: u32, h: u32) -> Quad {
        let (w, h) = (w as f32, h as f32);
        Quad([
            Point { x: 0.0, y: 0.0 },
            Point { x: w, y: 0.0 },
            Point { x: w, y: h },
            Point { x: 0.0, y: h },
        ])
    }

    /// Reorders any four points into TL, TR, BR, BL: clockwise on screen,
    /// starting with the edge closest to horizontal as the top edge, so the
    /// labelling is stable under rotation.
    pub fn ordered(pts: [Point; 4]) -> Quad {
        let cx = pts.iter().map(|p| p.x).sum::<f32>() / 4.0;
        let cy = pts.iter().map(|p| p.y).sum::<f32>() / 4.0;
        let mut v = pts;
        // y grows downward, so increasing atan2 is clockwise on screen.
        v.sort_by(|a, b| {
            let aa = (a.y - cy).atan2(a.x - cx);
            let bb = (b.y - cy).atan2(b.x - cx);
            aa.total_cmp(&bb)
        });
        let start = (0..4)
            .min_by(|&i, &j| {
                let slope = |k: usize| {
                    let (a, b) = (v[k], v[(k + 1) % 4]);
                    (b.y - a.y).atan2(b.x - a.x).abs()
                };
                slope(i).total_cmp(&slope(j))
            })
            .unwrap_or(0);
        Quad([v[start], v[(start + 1) % 4], v[(start + 2) % 4], v[(start + 3) % 4]])
    }

    fn area(&self) -> f32 {
        poly_area(&self.0).abs()
    }
}

pub fn detect_page(img: &DynamicImage) -> Detection {
    let (w, h) = (img.width(), img.height());
    let fallback = Detection { quad: Quad::full(w, h), found: false, confidence: 0.0 };
    if w < 16 || h < 16 {
        return fallback;
    }
    let (small, scale) = downscale_gray(img);
    detect_small(&small)
        .map(|(mut quad, confidence)| {
            for p in quad.0.iter_mut() {
                p.x = (p.x * scale).clamp(0.0, w as f32);
                p.y = (p.y * scale).clamp(0.0, h as f32);
            }
            Detection { quad, found: true, confidence }
        })
        .unwrap_or(fallback)
}

/// Grayscale box-downscale so the long side is about `DETECT_SIZE`.
/// Returns the image and the factor mapping its coordinates back to the source.
fn downscale_gray(img: &DynamicImage) -> (GrayImage, f32) {
    let gray = img.to_luma8();
    let (w, h) = gray.dimensions();
    let f = (w.max(h) / DETECT_SIZE).max(1);
    if f == 1 {
        return (gray, 1.0);
    }
    let (nw, nh) = (w / f, h / f);
    let src = gray.as_raw();
    let mut out = GrayImage::new(nw, nh);
    for y in 0..nh {
        for x in 0..nw {
            let mut sum = 0u32;
            for dy in 0..f {
                let row = ((y * f + dy) * w + x * f) as usize;
                sum += src[row..row + f as usize].iter().map(|&v| v as u32).sum::<u32>();
            }
            out.put_pixel(x, y, image::Luma([(sum / (f * f)) as u8]));
        }
    }
    (out, f as f32)
}

fn detect_small(gray: &GrayImage) -> Option<(Quad, f32)> {
    let (w, h) = (gray.width() as usize, gray.height() as usize);
    let blurred = box_blur(gray.as_raw(), w, h, 2);
    let t = otsu(&blurred);
    let mask: Vec<bool> = blurred.iter().map(|&v| v > t).collect();
    let (blob, area) = largest_component(&mask, w, h)?;

    let total = (w * h) as f32;
    if (area as f32) < MIN_AREA_FRAC * total {
        return None;
    }
    let hull = convex_hull(&boundary(&blob, w, h));
    if hull.len() < 4 {
        return None;
    }
    let corners = reduce_to_quad(hull)?;
    let quad = Quad::ordered(corners);

    let qa = quad.area();
    if qa < MIN_AREA_FRAC * total || qa > MAX_AREA_FRAC * total * 1.05 {
        return None;
    }
    let fill = area as f32 / qa;
    if fill < MIN_FILL || !plausible_shape(&quad) {
        return None;
    }
    // The circumscribed quad can poke past the frame when the page is cut off;
    // judge "whole frame" by the clamped quad.
    let mut clamped = quad;
    for p in clamped.0.iter_mut() {
        p.x = p.x.clamp(0.0, w as f32);
        p.y = p.y.clamp(0.0, h as f32);
    }
    if clamped.area() > MAX_AREA_FRAC * total {
        return None;
    }
    Some((clamped, fill.min(1.0)))
}

/// Convex, with interior angles in 40..140 degrees and no tiny edge.
fn plausible_shape(q: &Quad) -> bool {
    let p = &q.0;
    let mut sign = 0.0f32;
    for i in 0..4 {
        let (a, b, c) = (p[i], p[(i + 1) % 4], p[(i + 2) % 4]);
        let (ux, uy, vx, vy) = (a.x - b.x, a.y - b.y, c.x - b.x, c.y - b.y);
        let cross = ux * vy - uy * vx;
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
        let lu = (ux * ux + uy * uy).sqrt();
        let lv = (vx * vx + vy * vy).sqrt();
        if lu < 8.0 || lv < 8.0 {
            return false;
        }
        let cos = (ux * vx + uy * vy) / (lu * lv);
        if !(-0.77..=0.77).contains(&cos) {
            return false;
        }
    }
    true
}

fn box_blur(src: &[u8], w: usize, h: usize, r: usize) -> Vec<u8> {
    // Separable running-sum blur with edge clamping.
    let n = (2 * r + 1) as u32;
    let mut tmp = vec![0u8; w * h];
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        let at = |i: isize| row[i.clamp(0, w as isize - 1) as usize] as u32;
        let mut sum: u32 = (-(r as isize)..=r as isize).map(at).sum();
        for x in 0..w {
            tmp[y * w + x] = (sum / n) as u8;
            sum = sum + at((x + r + 1) as isize) - at(x as isize - r as isize);
        }
    }
    let mut out = vec![0u8; w * h];
    for x in 0..w {
        let at = |i: isize| tmp[i.clamp(0, h as isize - 1) as usize * w + x] as u32;
        let mut sum: u32 = (-(r as isize)..=r as isize).map(at).sum();
        for y in 0..h {
            out[y * w + x] = (sum / n) as u8;
            sum = sum + at((y + r + 1) as isize) - at(y as isize - r as isize);
        }
    }
    out
}

fn otsu(data: &[u8]) -> u8 {
    let mut hist = [0u64; 256];
    for &v in data {
        hist[v as usize] += 1;
    }
    let total = data.len() as f64;
    let sum_all: f64 = hist.iter().enumerate().map(|(i, &c)| i as f64 * c as f64).sum();
    let (mut w0, mut sum0, mut best, mut best_t) = (0.0f64, 0.0f64, -1.0f64, 0u8);
    for (t, &c) in hist.iter().enumerate() {
        w0 += c as f64;
        if w0 == 0.0 {
            continue;
        }
        let w1 = total - w0;
        if w1 == 0.0 {
            break;
        }
        sum0 += t as f64 * c as f64;
        let (m0, m1) = (sum0 / w0, (sum_all - sum0) / w1);
        let between = w0 * w1 * (m0 - m1) * (m0 - m1);
        if between > best {
            best = between;
            best_t = t as u8;
        }
    }
    best_t
}

/// Largest 4-connected component of true pixels, as a mask plus its pixel count.
fn largest_component(mask: &[bool], w: usize, h: usize) -> Option<(Vec<bool>, usize)> {
    let mut label = vec![0u32; w * h];
    let mut sizes = vec![0usize];
    let mut stack = Vec::new();
    for start in 0..w * h {
        if !mask[start] || label[start] != 0 {
            continue;
        }
        let id = sizes.len() as u32;
        let mut count = 0;
        label[start] = id;
        stack.push(start);
        while let Some(i) = stack.pop() {
            count += 1;
            let (x, y) = (i % w, i / w);
            let mut visit = |j: usize| {
                if mask[j] && label[j] == 0 {
                    label[j] = id;
                    stack.push(j);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < w {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - w);
            }
            if y + 1 < h {
                visit(i + w);
            }
        }
        sizes.push(count);
    }
    let (best, &area) = sizes.iter().enumerate().skip(1).max_by_key(|&(_, &s)| s)?;
    let blob = label.iter().map(|&l| l == best as u32).collect();
    Some((blob, area))
}

/// Pixel-corner coordinates of blob pixels that touch the outside.
fn boundary(blob: &[bool], w: usize, h: usize) -> Vec<(i64, i64)> {
    let mut pts = Vec::new();
    for y in 0..h {
        for x in 0..w {
            if !blob[y * w + x] {
                continue;
            }
            let edge = x == 0
                || y == 0
                || x + 1 == w
                || y + 1 == h
                || !blob[y * w + x - 1]
                || !blob[y * w + x + 1]
                || !blob[(y - 1) * w + x]
                || !blob[(y + 1) * w + x];
            if edge {
                // Use both pixel corners so the hull covers the pixel's full extent.
                let (x, y) = (x as i64, y as i64);
                pts.extend([(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)]);
            }
        }
    }
    pts
}

/// Andrew's monotone chain.
fn convex_hull(pts: &[(i64, i64)]) -> Vec<Point> {
    let mut p = pts.to_vec();
    p.sort_unstable();
    p.dedup();
    if p.len() < 3 {
        return Vec::new();
    }
    let cross = |o: (i64, i64), a: (i64, i64), b: (i64, i64)| {
        (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
    };
    let mut hull: Vec<(i64, i64)> = Vec::new();
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &(i64, i64)>> =
            if pass == 0 { Box::new(p.iter()) } else { Box::new(p.iter().rev()) };
        for &q in iter {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], q) <= 0
            {
                hull.pop();
            }
            hull.push(q);
        }
        hull.pop();
    }
    hull.into_iter().map(|(x, y)| Point { x: x as f32, y: y as f32 }).collect()
}

fn poly_area(p: &[Point]) -> f32 {
    let n = p.len();
    (0..n)
        .map(|i| p[i].x * p[(i + 1) % n].y - p[(i + 1) % n].x * p[i].y)
        .sum::<f32>()
        / 2.0
}

fn intersect(a: Point, b: Point, c: Point, d: Point) -> Option<Point> {
    let (r_x, r_y, s_x, s_y) = (b.x - a.x, b.y - a.y, d.x - c.x, d.y - c.y);
    let den = r_x * s_y - r_y * s_x;
    if den.abs() < 1e-6 {
        return None;
    }
    let t = ((c.x - a.x) * s_y - (c.y - a.y) * s_x) / den;
    Some(Point { x: a.x + t * r_x, y: a.y + t * r_y })
}

/// Repeatedly drops the hull edge whose removal (extending its two neighbours
/// until they meet) adds the least area, until four corners remain.
fn reduce_to_quad(mut v: Vec<Point>) -> Option<[Point; 4]> {
    let orient = poly_area(&v).signum();
    while v.len() > 4 {
        let n = v.len();
        let mut best: Option<(f32, usize, Point)> = None;
        for i in 0..n {
            let (p0, p1, p2, p3) = (v[(i + n - 1) % n], v[i], v[(i + 1) % n], v[(i + 2) % n]);
            let Some(x) = intersect(p0, p1, p3, p2) else { continue };
            // The meeting point must lie outside the removed edge.
            let side = (p2.x - p1.x) * (x.y - p1.y) - (p2.y - p1.y) * (x.x - p1.x);
            if side * orient >= 0.0 {
                continue;
            }
            let added = poly_area(&[p1, x, p2]).abs();
            if best.is_none_or(|(a, _, _)| added < a) {
                best = Some((added, i, x));
            }
        }
        let (_, i, x) = best?;
        let j = (i + 1) % n;
        v[i] = x;
        v.remove(j);
    }
    (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
}

// ---------------------------------------------------------------- warp

/// Flattens `quad` (TL, TR, BR, BL) into a rectangle whose size follows the
/// quad's edge lengths, sampling the source bilinearly.
pub fn warp(img: &DynamicImage, quad: &Quad) -> DynamicImage {
    let src = img.to_rgb8();
    let q = &quad.0;
    let dist = |a: Point, b: Point| ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
    let out_w = ((dist(q[0], q[1]) + dist(q[3], q[2])) / 2.0).round().max(1.0) as u32;
    let out_h = ((dist(q[0], q[3]) + dist(q[1], q[2])) / 2.0).round().max(1.0) as u32;

    let (fw, fh) = (out_w as f64, out_h as f64);
    let dst = [(0.0, 0.0), (fw, 0.0), (fw, fh), (0.0, fh)];
    let Some(hm) = homography(&dst, q) else {
        return DynamicImage::ImageRgb8(src);
    };

    let mut out = RgbImage::new(out_w, out_h);
    for y in 0..out_h {
        for x in 0..out_w {
            let (u, v) = (x as f64 + 0.5, y as f64 + 0.5);
            let d = hm[6] * u + hm[7] * v + 1.0;
            let sx = (hm[0] * u + hm[1] * v + hm[2]) / d - 0.5;
            let sy = (hm[3] * u + hm[4] * v + hm[5]) / d - 0.5;
            out.put_pixel(x, y, sample(&src, sx, sy));
        }
    }
    DynamicImage::ImageRgb8(out)
}

fn sample(img: &RgbImage, x: f64, y: f64) -> Rgb<u8> {
    let (w, h) = (img.width() as i64, img.height() as i64);
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let px = |dx: i64, dy: i64| {
        let xi = (x0 as i64 + dx).clamp(0, w - 1) as u32;
        let yi = (y0 as i64 + dy).clamp(0, h - 1) as u32;
        img.get_pixel(xi, yi).0
    };
    let (a, b, c, d) = (px(0, 0), px(1, 0), px(0, 1), px(1, 1));
    let mut o = [0u8; 3];
    for k in 0..3 {
        let top = a[k] as f64 * (1.0 - fx) + b[k] as f64 * fx;
        let bot = c[k] as f64 * (1.0 - fx) + d[k] as f64 * fx;
        o[k] = (top * (1.0 - fy) + bot * fy).round() as u8;
    }
    Rgb(o)
}

/// Solves the 3x3 homography (h22 = 1) mapping each `from[i]` to `to[i]`.
fn homography(from: &[(f64, f64); 4], to: &[Point; 4]) -> Option<[f64; 8]> {
    let mut m = [[0.0f64; 9]; 8];
    for i in 0..4 {
        let (x, y) = from[i];
        let (u, v) = (to[i].x as f64, to[i].y as f64);
        m[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        m[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }
    for c in 0..8 {
        let p = (c..8).max_by(|&a, &b| m[a][c].abs().total_cmp(&m[b][c].abs()))?;
        if m[p][c].abs() < 1e-12 {
            return None;
        }
        m.swap(c, p);
        for r in 0..8 {
            if r != c {
                let f = m[r][c] / m[c][c];
                for k in c..9 {
                    m[r][k] -= f * m[c][k];
                }
            }
        }
    }
    let mut h = [0.0; 8];
    for i in 0..8 {
        h[i] = m[i][8] / m[i][i];
    }
    Some(h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    fn hash(x: u32, y: u32) -> u32 {
        let mut h = x.wrapping_mul(374761393) ^ y.wrapping_mul(668265263);
        h = (h ^ (h >> 13)).wrapping_mul(1274126177);
        h ^ (h >> 16)
    }

    fn inside(q: &[Point; 4], x: f32, y: f32) -> bool {
        let mut sign = 0.0;
        for i in 0..4 {
            let (a, b) = (q[i], q[(i + 1) % 4]);
            let c = (b.x - a.x) * (y - a.y) - (b.y - a.y) * (x - a.x);
            if c != 0.0 {
                if sign == 0.0 {
                    sign = c.signum();
                } else if c.signum() != sign {
                    return false;
                }
            }
        }
        true
    }

    /// Light page (with dark "text" lines) on a dark noisy background.
    fn scene(w: u32, h: u32, page: &[Point; 4]) -> DynamicImage {
        let img = RgbImage::from_fn(w, h, |x, y| {
            let n = hash(x, y) % 40;
            if inside(page, x as f32 + 0.5, y as f32 + 0.5) {
                let ink = (y / 3) % 9 == 0 && !hash(x / 6, y).is_multiple_of(3);
                let v = if ink { 60 } else { 215 + n as u8 / 2 };
                Rgb([v, v, v])
            } else {
                let v = 25 + n as u8 + ((x / 40 + y / 40) % 2 * 20) as u8;
                Rgb([v, v - 5, v])
            }
        });
        DynamicImage::ImageRgb8(img)
    }

    fn assert_close(q: &Quad, want: &[Point; 4], tol: f32) {
        for (g, w) in q.0.iter().zip(want) {
            let d = ((g.x - w.x).powi(2) + (g.y - w.y).powi(2)).sqrt();
            assert!(d <= tol, "corner {g:?} not within {tol}px of {w:?}: {q:?}");
        }
    }

    #[test]
    fn detects_perspective_quad() {
        let page = [pt(150.0, 90.0), pt(660.0, 130.0), pt(700.0, 520.0), pt(110.0, 470.0)];
        let d = detect_page(&scene(800, 600, &page));
        assert!(d.found);
        assert_close(&d.quad, &page, 5.0);
    }

    #[test]
    fn detects_rotated_page_with_consistent_order() {
        let (cx, cy, hw, hh) = (400.0f32, 300.0, 250.0, 180.0);
        let mut seen = Vec::new();
        for deg in [-30.0f32, -10.0, 0.0, 15.0, 35.0] {
            let (s, c) = deg.to_radians().sin_cos();
            // Corners in TL, TR, BR, BL order before rotation.
            let page = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)]
                .map(|(x, y)| pt(cx + x * c - y * s, cy + x * s + y * c));
            let d = detect_page(&scene(800, 600, &page));
            assert!(d.found, "not found at {deg} degrees");
            assert_close(&d.quad, &page, 5.0);
            seen.push(d.quad);
        }
        assert_eq!(seen.len(), 5);
    }

    #[test]
    fn no_page_returns_full_image() {
        // Noise only.
        let noise = RgbImage::from_fn(640, 480, |x, y| {
            let v = 90 + (hash(x, y) % 60) as u8;
            Rgb([v, v, v])
        });
        // A flat white frame (page fills everything).
        let white = RgbImage::from_pixel(640, 480, Rgb([230, 230, 230]));
        for img in [noise, white] {
            let d = detect_page(&DynamicImage::ImageRgb8(img));
            assert!(!d.found);
            assert_eq!(d.quad, Quad::full(640, 480));
        }
    }

    #[test]
    fn tiny_image_does_not_panic() {
        let d = detect_page(&DynamicImage::ImageRgb8(RgbImage::new(3, 3)));
        assert!(!d.found);
    }

    #[test]
    fn ordering_is_rotation_stable() {
        let base = [pt(100.0, 100.0), pt(300.0, 110.0), pt(310.0, 400.0), pt(90.0, 390.0)];
        let want = Quad(base);
        for rot in 0..4 {
            let mut p = base;
            p.rotate_left(rot);
            p.swap(0, 2); // shuffle further
            assert_eq!(Quad::ordered(p), want);
        }
    }

    #[test]
    fn warp_flattens_checkerboard() {
        // Page rotated 20 degrees, checkerboard (20px squares) defined in page coords.
        let (pw, ph) = (300.0f32, 200.0f32);
        let (cx, cy) = (300.0f32, 220.0f32);
        let (s, c) = 20f32.to_radians().sin_cos();
        let to_img = |u: f32, v: f32| {
            let (x, y) = (u - pw / 2.0, v - ph / 2.0);
            pt(cx + x * c - y * s, cy + x * s + y * c)
        };
        let quad = Quad([to_img(0.0, 0.0), to_img(pw, 0.0), to_img(pw, ph), to_img(0.0, ph)]);
        let src = RgbImage::from_fn(600, 440, |x, y| {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let u = dx * c + dy * s + pw / 2.0;
            let v = -dx * s + dy * c + ph / 2.0;
            let on = ((u / 20.0).floor() as i32 + (v / 20.0).floor() as i32).rem_euclid(2) == 0;
            let g = if on { 240 } else { 20 };
            Rgb([g, g, g])
        });
        let out = warp(&DynamicImage::ImageRgb8(src), &quad).to_rgb8();
        assert!((out.width() as i32 - 300).abs() <= 1 && (out.height() as i32 - 200).abs() <= 1);

        let (mut bad, mut total) = (0, 0);
        for y in 0..out.height() {
            for x in 0..out.width() {
                // Skip pixels near square borders where either side may differ.
                let (fx, fy) = (x % 20, y % 20);
                if !(3..17).contains(&fx) || !(3..17).contains(&fy) {
                    continue;
                }
                let on = (x / 20 + y / 20) % 2 == 0;
                let v = out.get_pixel(x, y).0[0];
                total += 1;
                if (v > 128) != on {
                    bad += 1;
                }
            }
        }
        assert!(bad * 100 <= total, "{bad}/{total} checkerboard pixels wrong");
    }

    #[test]
    fn warp_maps_perspective_corners() {
        // Four corner colours of a skewed quad must land in the output corners.
        let quad = Quad([pt(50.0, 40.0), pt(350.0, 70.0), pt(380.0, 270.0), pt(30.0, 250.0)]);
        let src = RgbImage::from_fn(400, 300, |x, y| {
            let p = pt(x as f32 + 0.5, y as f32 + 0.5);
            let near = |q: Point| (p.x - q.x).abs() < 14.0 && (p.y - q.y).abs() < 14.0;
            if near(quad.0[0]) {
                Rgb([255, 0, 0])
            } else if near(quad.0[1]) {
                Rgb([0, 255, 0])
            } else if near(quad.0[2]) {
                Rgb([0, 0, 255])
            } else if near(quad.0[3]) {
                Rgb([255, 255, 0])
            } else {
                Rgb([128, 128, 128])
            }
        });
        let out = warp(&DynamicImage::ImageRgb8(src), &quad).to_rgb8();
        let (w, h) = (out.width(), out.height());
        assert_eq!(out.get_pixel(2, 2).0, [255, 0, 0]);
        assert_eq!(out.get_pixel(w - 3, 2).0, [0, 255, 0]);
        assert_eq!(out.get_pixel(w - 3, h - 3).0, [0, 0, 255]);
        assert_eq!(out.get_pixel(2, h - 3).0, [255, 255, 0]);
    }

    #[test]
    #[ignore = "timing; run with --release -- --ignored --nocapture"]
    fn timing_12mp() {
        let page = [pt(600.0, 450.0), pt(3200.0, 300.0), pt(3500.0, 2700.0), pt(400.0, 2800.0)];
        let img = scene(4000, 3000, &page);
        let t = std::time::Instant::now();
        let d = detect_page(&img);
        println!("12MP detect: {:?} found={}", t.elapsed(), d.found);
        assert!(d.found);
    }
}
