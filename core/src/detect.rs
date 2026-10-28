//! Page detection and perspective correction.
//!
//! `detect_page` only *suggests* corners: when unsure it returns the full-image
//! corners with `found = false` and never errors. `warp` flattens a quad into a
//! rectangle at the source resolution.
//!
//! Detection finds the page boundary as four straight lines: edge candidates
//! come from a Hough transform on a blurred colour copy, a candidate counts
//! only where the colour really changes across it (so ruled lines and cables
//! score nothing), and the quad whose sides are best supported wins. Anything
//! unconvincing falls back to the full image.

use image::{DynamicImage, Rgb, RgbImage};

/// Smallest accepted page, as a fraction of the image area.
const MIN_AREA_FRAC: f32 = 0.10;

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
        Quad([
            v[start],
            v[(start + 1) % 4],
            v[(start + 2) % 4],
            v[(start + 3) % 4],
        ])
    }

    fn area(&self) -> f32 {
        poly_area(&self.0).abs()
    }
}

pub fn detect_page(img: &DynamicImage) -> Detection {
    let (w, h) = (img.width(), img.height());
    let fallback = Detection {
        quad: Quad::full(w, h),
        found: false,
        confidence: 0.0,
    };
    if w < 16 || h < 16 {
        return fallback;
    }
    let found = detect_lines(img);
    found
        .map(|(mut quad, confidence)| {
            for p in quad.0.iter_mut() {
                p.x = p.x.clamp(0.0, w as f32);
                p.y = p.y.clamp(0.0, h as f32);
            }
            Detection {
                quad,
                found: true,
                confidence,
            }
        })
        .unwrap_or(fallback)
}

/// Grayscale box-downscale so the long side is about `DETECT_SIZE`.
/// Returns the image and the factor mapping its coordinates back to the source.
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
    Some(Point {
        x: a.x + t * r_x,
        y: a.y + t * r_y,
    })
}

/// Repeatedly drops the hull edge whose removal (extending its two neighbours
/// until they meet) adds the least area, until four corners remain.
// ------------------------------------------------- line-based detection
//
// Works on a colour working copy. Candidate page sides come from an oriented
// Hough transform over strong gradients; a candidate only survives if the
// colour just inside differs from the colour just outside along it (measured
// at an offset from the edge, so thin ruled lines and cables, which have
// paper or comforter on both sides, score nothing). Quads are then built from
// pairs of near-horizontal and near-vertical lines, plus the four frame edges
// for pages cut off by the photo, and scored by how much of each side is
// supported by such contrast.

/// Target long side of the colour working copy for line detection.
const LINE_WORK_SIZE: u32 = 800;
/// Offset of the inside/outside colour samples from a candidate edge, in
/// working-copy pixels. Larger than a ruled line plus its blur.
const SIDE_GAP: f32 = 8.0;
/// Mean RGB difference across an edge that counts as contrast.
const EDGE_CONTRAST: f32 = 12.0;
/// Score credited to a quad side lying on the photo frame (page cut off).
const FRAME_SIDE_SCORE: f32 = 0.45;
/// Lowest score any single side may have; a quad with an unsupported side is
/// not a page, however good the other three are.
const MIN_SIDE_SCORE: f32 = 0.3;
/// Lowest mean side score accepted as a page.
const MIN_QUAD_SCORE: f32 = 0.55;
/// How far refinement may move a side end, working-copy pixels.
const REFINE_RANGE: i32 = 10;
/// Offset of the colour samples used to place a side precisely.
const REFINE_GAP: f32 = 4.0;
/// Gray levels of contrast one pixel of movement must buy.
const REFINE_PENALTY: f32 = 0.15;
/// Smallest accumulator weight worth considering as a line.
const MIN_HOUGH_WEIGHT: f32 = 40.0;
/// Two peaks are one line if they cross the image within this many pixels at
/// both ends and the middle.
const SAME_LINE_PX: f32 = 6.0;
/// Most Hough peaks given the (comparatively slow) contrast test, per
/// orientation; ruled lines can fill the top of the list.
const MAX_TESTED_PEAKS: usize = 800;
/// Hough angle window around horizontal/vertical, degrees.
const ANGLE_SLACK: i32 = 40;
/// Candidate lines kept per orientation after the contrast test.
const MAX_LINES: usize = 12;

struct Rgbf {
    w: usize,
    h: usize,
    d: Vec<[f32; 3]>,
}

impl Rgbf {
    fn at(&self, x: usize, y: usize) -> [f32; 3] {
        self.d[y * self.w + x]
    }

    /// Bilinear sample; None outside the image.
    fn sample(&self, x: f32, y: f32) -> Option<[f32; 3]> {
        if x < 0.0 || y < 0.0 || x > (self.w - 1) as f32 || y > (self.h - 1) as f32 {
            return None;
        }
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let mut o = [0.0; 3];
        for (k, ok) in o.iter_mut().enumerate() {
            let top = self.at(x0, y0)[k] * (1.0 - fx) + self.at(x1, y0)[k] * fx;
            let bot = self.at(x0, y1)[k] * (1.0 - fx) + self.at(x1, y1)[k] * fx;
            *ok = top * (1.0 - fy) + bot * fy;
        }
        Some(o)
    }
}

/// Box-downscale to about `LINE_WORK_SIZE`, then a light 3x3 blur.
fn downscale_rgb(img: &DynamicImage) -> (Rgbf, f32) {
    let rgb = img.to_rgb8();
    let (w, h) = rgb.dimensions();
    let f = w.max(h).div_ceil(LINE_WORK_SIZE).max(1) as usize;
    let (nw, nh) = (w as usize / f, h as usize / f);
    let src = rgb.as_raw();
    let mut d = vec![[0.0f32; 3]; nw * nh];
    for y in 0..nh {
        for x in 0..nw {
            let mut s = [0u32; 3];
            for dy in 0..f {
                for dx in 0..f {
                    let i = ((y * f + dy) * w as usize + x * f + dx) * 3;
                    for k in 0..3 {
                        s[k] += src[i + k] as u32;
                    }
                }
            }
            let n = (f * f) as f32;
            d[y * nw + x] = [s[0] as f32 / n, s[1] as f32 / n, s[2] as f32 / n];
        }
    }
    let raw = Rgbf { w: nw, h: nh, d };
    let mut out = vec![[0.0f32; 3]; nw * nh];
    for y in 0..nh {
        for x in 0..nw {
            let mut s = [0.0f32; 3];
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let p = raw.at(
                        (x as i32 + dx).clamp(0, nw as i32 - 1) as usize,
                        (y as i32 + dy).clamp(0, nh as i32 - 1) as usize,
                    );
                    for k in 0..3 {
                        s[k] += p[k] / 9.0;
                    }
                }
            }
            out[y * nw + x] = s;
        }
    }
    (
        Rgbf {
            w: nw,
            h: nh,
            d: out,
        },
        f as f32,
    )
}

/// Separable box blur of radius `r` with edge clamping.
fn blur_rgb(src: &Rgbf, r: i32) -> Rgbf {
    let (w, h) = (src.w, src.h);
    let n = (2 * r + 1) as f32;
    let mut tmp = vec![[0.0f32; 3]; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut s = [0.0f32; 3];
            for d in -r..=r {
                let p = src.at((x as i32 + d).clamp(0, w as i32 - 1) as usize, y);
                for k in 0..3 {
                    s[k] += p[k];
                }
            }
            tmp[y * w + x] = [s[0] / n, s[1] / n, s[2] / n];
        }
    }
    let mut out = vec![[0.0f32; 3]; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut s = [0.0f32; 3];
            for d in -r..=r {
                let p = tmp[(y as i32 + d).clamp(0, h as i32 - 1) as usize * w + x];
                for k in 0..3 {
                    s[k] += p[k];
                }
            }
            out[y * w + x] = [s[0] / n, s[1] / n, s[2] / n];
        }
    }
    Rgbf { w, h, d: out }
}

/// A candidate side: the points `n * rho + dir * t` with `n = (cos, sin)`.
struct Line {
    cos: f32,
    sin: f32,
    rho: f32,
    frame: bool,
    /// Range of `t` where the line is inside the image.
    t0: i32,
    t1: i32,
    /// `strong[i]` = (inside brighter, inside darker) counts of strong-contrast
    /// samples among `t0..t0 + i`; a real edge has one sign all along.
    strong: Vec<[u32; 2]>,
}

impl Line {
    fn new(theta: f32, rho: f32, frame: bool, img: &Rgbf) -> Line {
        let (sin, cos) = theta.sin_cos();
        let mut line = Line {
            cos,
            sin,
            rho,
            frame,
            t0: 0,
            t1: -1,
            strong: vec![[0; 2]],
        };
        let (px, py) = (rho * cos, rho * sin);
        let (mut lo, mut hi) = (f32::MIN, f32::MAX);
        for (p, d, max) in [
            (px, -sin, (img.w - 1) as f32),
            (py, cos, (img.h - 1) as f32),
        ] {
            if d.abs() < 1e-6 {
                if p < -0.5 || p > max + 0.5 {
                    return line;
                }
            } else {
                let (a, b) = ((0.0 - p) / d, (max - p) / d);
                lo = lo.max(a.min(b));
                hi = hi.min(a.max(b));
            }
        }
        line.t0 = lo.ceil() as i32;
        line.t1 = hi.floor() as i32;
        if line.t1 < line.t0 || frame {
            return line;
        }
        let mut strong = Vec::with_capacity((line.t1 - line.t0 + 2) as usize);
        strong.push([0; 2]);
        let mut count = [0u32; 2];
        for t in line.t0..=line.t1 {
            let (x, y) = (px - sin * t as f32, py + cos * t as f32);
            let a = img.sample(x + cos * SIDE_GAP, y + sin * SIDE_GAP);
            let b = img.sample(x - cos * SIDE_GAP, y - sin * SIDE_GAP);
            if let (Some(a), Some(b)) = (a, b) {
                let diff = ((a[0] - b[0]).abs() + (a[1] - b[1]).abs() + (a[2] - b[2]).abs()) / 3.0;
                if diff > EDGE_CONTRAST {
                    let lum = |p: [f32; 3]| p[0] + p[1] + p[2];
                    count[(lum(a) < lum(b)) as usize] += 1;
                }
            }
            strong.push(count);
        }
        line.strong = strong;
        line
    }

    /// Share of samples `i..j` with strong contrast of the dominant sign.
    fn run(&self, i: usize, j: usize) -> f32 {
        let d = |k: usize| self.strong[j][k] - self.strong[i][k];
        d(0).max(d(1)) as f32 / (j - i) as f32
    }

    fn t_of(&self, p: Point) -> f32 {
        -self.sin * (p.x - self.rho * self.cos) + self.cos * (p.y - self.rho * self.sin)
    }

    /// Fraction of the side between `a` and `b` (both on this line) backed by
    /// edge contrast; 0 if too little of it is inside the image to judge.
    fn coverage(&self, a: Point, b: Point) -> f32 {
        if self.frame {
            return FRAME_SIDE_SCORE;
        }
        let (ta, tb) = (self.t_of(a), self.t_of(b));
        let (lo, hi) = (ta.min(tb), ta.max(tb));
        let len = (hi - lo).max(1.0);
        let (clo, chi) = (lo.max(self.t0 as f32), hi.min(self.t1 as f32));
        if chi - clo < 0.5 * len {
            return 0.0;
        }
        let i = (clo - self.t0 as f32) as usize;
        let j = ((chi - self.t0 as f32) as usize + 1).min(self.strong.len() - 1);
        if j <= i {
            return 0.0;
        }
        self.run(i, j)
    }

    /// Best contrast coverage over any run of `win` samples; rejects lines
    /// that never separate two different surfaces.
    fn best_window(&self, win: usize) -> f32 {
        let n = self.strong.len() - 1;
        if n < win {
            return 0.0;
        }
        (0..=n - win)
            .map(|i| self.run(i, i + win))
            .fold(0.0, f32::max)
    }

    fn y_at_x(&self, x: f32) -> f32 {
        (self.rho - x * self.cos) / self.sin
    }

    fn x_at_y(&self, y: f32) -> f32 {
        (self.rho - y * self.sin) / self.cos
    }
}

fn dist(a: Point, b: Point) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

fn intersect_lines(a: &Line, b: &Line) -> Option<Point> {
    // cos*x + sin*y = rho for both.
    let det = a.cos * b.sin - a.sin * b.cos;
    if det.abs() < 1e-3 {
        return None;
    }
    Some(Point {
        x: (a.rho * b.sin - a.sin * b.rho) / det,
        y: (a.cos * b.rho - a.rho * b.cos) / det,
    })
}

/// Oriented Hough peaks for lines whose normal is within `ANGLE_SLACK` of
/// vertical (horizontal lines) or horizontal (vertical lines), keeping those
/// with real edge contrast.
fn hough_lines(smooth: &Rgbf, horizontal: bool, count: usize) -> Vec<Line> {
    let (w, h) = (smooth.w, smooth.h);
    let lum = |x: usize, y: usize| {
        let p = smooth.at(x, y);
        (p[0] + p[1] + p[2]) / 3.0
    };
    let rho_off = (w + h) as f32;
    let nrho = 2 * (w + h) + 2;
    let allowed = |deg: i32| {
        let d = deg.rem_euclid(180);
        if horizontal {
            (d - 90).abs() <= ANGLE_SLACK
        } else {
            d <= ANGLE_SLACK || d >= 180 - ANGLE_SLACK
        }
    };
    let (sins, coss): (Vec<f32>, Vec<f32>) =
        (0..180).map(|d| (d as f32).to_radians().sin_cos()).unzip();
    let mut acc = vec![0.0f32; 180 * nrho];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let gx = lum(x + 1, y - 1) + 2.0 * lum(x + 1, y) + lum(x + 1, y + 1)
                - lum(x - 1, y - 1)
                - 2.0 * lum(x - 1, y)
                - lum(x - 1, y + 1);
            let gy = lum(x - 1, y + 1) + 2.0 * lum(x, y + 1) + lum(x + 1, y + 1)
                - lum(x - 1, y - 1)
                - 2.0 * lum(x, y - 1)
                - lum(x + 1, y - 1);
            let mag = (gx * gx + gy * gy).sqrt() / 4.0;
            if mag < 8.0 {
                continue;
            }
            let theta0 = gy.atan2(gx).to_degrees().round() as i32;
            for dth in -2..=2 {
                let deg = theta0 + dth;
                if !allowed(deg) {
                    continue;
                }
                let di = deg.rem_euclid(180) as usize;
                let rho = x as f32 * coss[di] + y as f32 * sins[di] + rho_off;
                let ri = rho.round() as usize;
                if ri < nrho {
                    acc[di * nrho + ri] += mag * (1.0 - dth.abs() as f32 * 0.2);
                }
            }
        }
    }
    // Local maxima of the accumulator, strongest first. Neighbouring bins are
    // only compared for the peak test; whether two peaks are the same line is
    // decided below by where they cross the image, because bins near the image
    // origin are close in rho for lines of quite different angle.
    let mut peaks: Vec<(f32, usize, usize)> = Vec::new();
    for di in 0..180usize {
        if !allowed(di as i32) {
            continue;
        }
        for ri in 2..nrho - 2 {
            let v = acc[di * nrho + ri];
            if v < MIN_HOUGH_WEIGHT {
                continue;
            }
            let is_max = (-1i32..=1).all(|dd| {
                let d2 = (di as i32 + dd).rem_euclid(180) as usize;
                (-2i32..=2).all(|dr| acc[d2 * nrho + (ri as i32 + dr) as usize] <= v)
            });
            if is_max {
                peaks.push((v, di, ri));
            }
        }
    }
    peaks.sort_by(|a, b| b.0.total_cmp(&a.0));

    let win = (((if horizontal { w } else { h }) as f32 * 0.25) as usize).max(8);
    let (len, far) = if horizontal { (w as f32, h as f32) } else { (h as f32, w as f32) };
    // Where a line crosses the image at three stations along its length.
    let stations = |cos: f32, sin: f32, rho: f32| {
        [0.0, len / 2.0, len].map(|s| if horizontal { (rho - s * cos) / sin } else { (rho - s * sin) / cos })
    };
    let mut lines: Vec<Line> = Vec::new();
    let mut kept: Vec<[f32; 3]> = Vec::new();
    let mut tested = 0;
    for &(_, di, ri) in &peaks {
        let rho = ri as f32 - rho_off;
        let st = stations(coss[di], sins[di], rho);
        if st.iter().all(|s| s.abs() > far * 4.0) {
            continue;
        }
        if kept.iter().any(|k| k.iter().zip(&st).all(|(a, b)| (a - b).abs() < SAME_LINE_PX)) {
            continue;
        }
        tested += 1;
        let line = Line::new((di as f32).to_radians(), rho, false, smooth);
        if line.best_window(win) >= 0.6 {
            kept.push(st);
            lines.push(line);
            if lines.len() >= count {
                break;
            }
        }
        if tested >= MAX_TESTED_PEAKS {
            break;
        }
    }
    lines
}

/// Candidate lines come from 1-degree Hough bins, which over a long side
/// leaves several pixels of tilt error. This slides the two endpoints of the
/// side `a`-`b` along its normal to wherever the colour step across the side
/// is strongest on average, preferring small moves.
fn refine_side(img: &Rgbf, a: Point, b: Point) -> (Point, Point) {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 24.0 {
        return (a, b);
    }
    let (nx, ny) = (-dy / len, dx / len);
    let n = len as usize;
    let r = REFINE_RANGE;
    let width = (2 * r + 1) as usize;
    let lum = |p: [f32; 3]| (p[0] + p[1] + p[2]) / 3.0;
    // table[i * width + s]: signed step across the side at sample i when the
    // side is shifted by s - r pixels along its normal.
    let mut table = vec![0.0f32; n * width];
    let mut valid = vec![false; n];
    for i in 0..n {
        let t = i as f32 / n as f32;
        let (px, py) = (a.x + dx * t, a.y + dy * t);
        for s in 0..width {
            let off = s as f32 - r as f32;
            let (cx, cy) = (px + nx * off, py + ny * off);
            let p = img.sample(cx + nx * REFINE_GAP, cy + ny * REFINE_GAP);
            let q = img.sample(cx - nx * REFINE_GAP, cy - ny * REFINE_GAP);
            if let (Some(p), Some(q)) = (p, q) {
                table[i * width + s] = lum(p) - lum(q);
                if s == r as usize {
                    valid[i] = true;
                }
            }
        }
    }
    if valid.iter().filter(|&&v| v).count() * 10 < n * 7 {
        return (a, b);
    }
    let mut best = (f32::MIN, 0, 0);
    for da in -r..=r {
        for db in -r..=r {
            let mut sum = 0.0;
            for i in 0..n {
                let shift = da as f32 + (db - da) as f32 * i as f32 / n as f32;
                let s = (shift.round() as i32 + r) as usize;
                sum += table[i * width + s];
            }
            let score = sum.abs() / n as f32 - REFINE_PENALTY * (da.abs() + db.abs()) as f32 / 2.0;
            if score > best.0 {
                best = (score, da, db);
            }
        }
    }
    let (_, da, db) = best;
    (
        Point {
            x: a.x + nx * da as f32,
            y: a.y + ny * da as f32,
        },
        Point {
            x: b.x + nx * db as f32,
            y: b.y + ny * db as f32,
        },
    )
}

/// Re-fits the four sides of `q` (TL, TR, BR, BL) and re-intersects them.
/// Sides lying on the photo frame stay where they are.
fn refine_quad(img: &Rgbf, q: &Quad, frame: [bool; 4]) -> Option<Quad> {
    let p = &q.0;
    // top, right, bottom, left as endpoint pairs.
    let mut sides = [(p[0], p[1]), (p[1], p[2]), (p[3], p[2]), (p[0], p[3])];
    for (side, &on_frame) in sides.iter_mut().zip(&frame) {
        if !on_frame {
            *side = refine_side(img, side.0, side.1);
        }
    }
    let [t, r, b, l] = sides;
    let refined = Quad([
        intersect(t.0, t.1, l.0, l.1)?,
        intersect(t.0, t.1, r.0, r.1)?,
        intersect(b.0, b.1, r.0, r.1)?,
        intersect(b.0, b.1, l.0, l.1)?,
    ]);
    plausible_shape(&refined).then_some(refined)
}

fn detect_lines(img: &DynamicImage) -> Option<(Quad, f32)> {
    let (work, scale) = downscale_rgb(img);
    if work.w < 32 || work.h < 32 {
        return None;
    }
    let (w, h) = (work.w as f32, work.h as f32);
    let smooth = blur_rgb(&work, 3);
    let mut hor = hough_lines(&smooth, true, MAX_LINES);
    let mut ver = hough_lines(&smooth, false, MAX_LINES);
    hor.push(Line::new(std::f32::consts::FRAC_PI_2, 0.0, true, &smooth));
    hor.push(Line::new(
        std::f32::consts::FRAC_PI_2,
        h - 1.0,
        true,
        &smooth,
    ));
    ver.push(Line::new(0.0, 0.0, true, &smooth));
    ver.push(Line::new(0.0, w - 1.0, true, &smooth));
    // Order top to bottom and left to right at the image centre.
    hor.sort_by(|a, b| a.y_at_x(w / 2.0).total_cmp(&b.y_at_x(w / 2.0)));
    ver.sort_by(|a, b| a.x_at_y(h / 2.0).total_cmp(&b.x_at_y(h / 2.0)));

    let total = w * h;
    let slop = 0.02 * w.max(h);
    let mut best: Option<(f32, [Point; 4], [bool; 4], f32)> = None;
    for ti in 0..hor.len() {
        for bi in ti + 1..hor.len() {
            for li in 0..ver.len() {
                for ri in li + 1..ver.len() {
                    let (t, b, l, r) = (&hor[ti], &hor[bi], &ver[li], &ver[ri]);
                    if t.frame && b.frame && l.frame && r.frame {
                        continue;
                    }
                    let (Some(tl), Some(tr), Some(br), Some(bl)) = (
                        intersect_lines(t, l),
                        intersect_lines(t, r),
                        intersect_lines(b, r),
                        intersect_lines(b, l),
                    ) else {
                        continue;
                    };
                    let q = Quad([tl, tr, br, bl]);
                    if q.0
                        .iter()
                        .any(|p| p.x < -slop || p.y < -slop || p.x > w + slop || p.y > h + slop)
                    {
                        continue;
                    }
                    if q.area() < MIN_AREA_FRAC * total || !plausible_shape(&q) {
                        continue;
                    }
                    let sides = [
                        t.coverage(tl, tr),
                        r.coverage(tr, br),
                        b.coverage(bl, br),
                        l.coverage(tl, bl),
                    ];
                    if sides.iter().any(|&s| s < MIN_SIDE_SCORE) {
                        continue;
                    }
                    // Edge length the quad explains. Choosing by total length
                    // rather than mean coverage keeps a sub-rectangle of the
                    // page, whose shorter sides are just as well supported,
                    // from beating the whole page.
                    let lens = [dist(tl, tr), dist(tr, br), dist(bl, br), dist(tl, bl)];
                    let supported: f32 = sides.iter().zip(&lens).map(|(s, l)| s * l).sum();
                    let mean = supported / lens.iter().sum::<f32>();
                    if mean < MIN_QUAD_SCORE {
                        continue;
                    }
                    let score = supported * mean;
                    if best.is_none_or(|(s, _, _, _)| score > s) {
                        best = Some((score, q.0, [t.frame, r.frame, b.frame, l.frame], mean));
                    }
                }
            }
        }
    }
    let (_, pts, frame, mean) = best?;
    let mut quad = Quad(pts);
    if let Some(refined) = refine_quad(&smooth, &quad, frame) {
        quad = refined;
    }
    for p in quad.0.iter_mut() {
        p.x = (p.x * scale).max(0.0);
        p.y = (p.y * scale).max(0.0);
    }
    Some((quad, mean.min(1.0)))
}

// ---------------------------------------------------------------- warp

/// Flattens `quad` (TL, TR, BR, BL) into a rectangle whose size follows the
/// quad's edge lengths, sampling the source bilinearly.
pub fn warp(img: &DynamicImage, quad: &Quad) -> DynamicImage {
    let src = img.to_rgb8();
    let q = &quad.0;
    let dist = |a: Point, b: Point| ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
    let out_w = ((dist(q[0], q[1]) + dist(q[3], q[2])) / 2.0)
        .round()
        .max(1.0) as u32;
    let out_h = ((dist(q[0], q[3]) + dist(q[1], q[2])) / 2.0)
        .round()
        .max(1.0) as u32;

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
        let page = [
            pt(150.0, 90.0),
            pt(660.0, 130.0),
            pt(700.0, 520.0),
            pt(110.0, 470.0),
        ];
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
        let base = [
            pt(100.0, 100.0),
            pt(300.0, 110.0),
            pt(310.0, 400.0),
            pt(90.0, 390.0),
        ];
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
        let quad = Quad([
            to_img(0.0, 0.0),
            to_img(pw, 0.0),
            to_img(pw, ph),
            to_img(0.0, ph),
        ]);
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
        assert!(
            bad * 100 <= total,
            "{bad}/{total} checkerboard pixels wrong"
        );
    }

    #[test]
    fn warp_maps_perspective_corners() {
        // Four corner colours of a skewed quad must land in the output corners.
        let quad = Quad([
            pt(50.0, 40.0),
            pt(350.0, 70.0),
            pt(380.0, 270.0),
            pt(30.0, 250.0),
        ]);
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
        let page = [
            pt(600.0, 450.0),
            pt(3200.0, 300.0),
            pt(3500.0, 2700.0),
            pt(400.0, 2800.0),
        ];
        let img = scene(4000, 3000, &page);
        let t = std::time::Instant::now();
        let d = detect_page(&img);
        println!("12MP detect: {:?} found={}", t.elapsed(), d.found);
        assert!(d.found);
    }
}
