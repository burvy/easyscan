//! Annotation strokes composited over a page image.
//!
//! Pure and stateless (DESIGN.md): one image in, one image out. The caller
//! owns the coordinate frame; each stroke carries points normalized to
//! [0..1]² of that frame and a width as a fraction of the frame's short
//! side, so the same stroke data renders consistently at any resolution —
//! the app renders the editor preview in Dart and burns the export in Rust
//! from the very same normalized stroke lists.
//!
//! Coordinate convention matches `detect::warp`: positions live between
//! pixel corners, a pixel index x has its centre at x + 0.5, so
//! `pixel.x = nx * img.width()` means an `nx` of 1 lands on the right
//! edge of the last pixel column.

use image::{DynamicImage, Rgb, RgbImage};

/// Strokes are stamped *before* the scan filter runs (`filter::apply`), so
/// the filter decides how they render: the pen palette has to stay legible
/// binarized (dark ink colours survive BlackWhite, and nothing pale can be
/// eaten by the rule-line eraser), while white-out sampled from the paper
/// binarizes back to clean paper-white instead of leaving a warm tint
/// patch on a black-and-white page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Pen,
    Whiteout,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub tool: Tool,
    /// Opaque sRGB stroke colour: the ink for `Pen`, the sampled paper
    /// colour for `Whiteout`.
    pub color: [u8; 3],
    /// Line thickness as a fraction of `min(width, height)` of the frame.
    pub width: f64,
    /// Flat x0, y0, x1, y1, ... pairs in [0..1] of the frame. A stroke
    /// needs at least two points.
    pub points: Vec<f64>,
}

/// Width of the alpha ramp on each side of the line, in pixels.
const EDGE_SOFTNESS_PX: f64 = 1.5;

impl Stroke {
    /// Malformed data (not enough points, non-finite values) is dropped
    /// rather than trusted, mirroring the lenient JSON parser on the Dart
    /// side.
    fn is_sane(&self) -> bool {
        self.points.len() >= 4
            && self.points.iter().all(|v| v.is_finite())
            && self.width.is_finite()
    }
}

/// Composites `strokes` over `img`. Stroke pixels map to the frame as
/// `pixel.x = nx * img.width()`, `pixel.y = ny * img.height()`, with
/// `width` scaled by `min(img.width(), img.height())`.
///
/// `apply` always returns an RGB image (`warp` already produces one). With
/// no valid strokes the input is returned unchanged.
pub fn apply(img: &DynamicImage, strokes: &[Stroke]) -> DynamicImage {
    let strokes: Vec<&Stroke> = strokes.iter().filter(|s| s.is_sane()).collect();
    if strokes.is_empty() {
        return img.clone();
    }
    let buf = img.to_rgb8();
    let mut buf = buf;
    if buf.width() == 0 || buf.height() == 0 {
        return DynamicImage::ImageRgb8(buf);
    }
    let (w, h) = (buf.width() as f64, buf.height() as f64);
    for s in &strokes {
        stamp_stroke(&mut buf, s, w, h);
    }
    DynamicImage::ImageRgb8(buf)
}

/// Paints one stroke into `buf`.
///
/// The stroke's ink footprint is the union of capsules (segments thickened
/// by `radius` discs), so each pixel's coverage is computed directly as the
/// signed distance to the *nearest* segment — a perfect `max()` union with
/// no stamp spacing and no per-disc accumulation. Work is bounded by the
/// stroke's own bounding box: one scratch mask of that box, one capsule
/// raster per segment, one blend pass.
fn stamp_stroke(buf: &mut RgbImage, s: &Stroke, w: f64, h: f64) {
    let (iw, ih) = (buf.width(), buf.height());
    let min_side = w.min(h);
    let radius = (s.width * min_side / 2.0)
        .abs()
        .max(0.5)
        .min(min_side / 4.0);
    let reach = radius + EDGE_SOFTNESS_PX;
    let points: Vec<(f64, f64)> = s
        .points
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[x, y]| (x * w, y * h))
        .collect();

    // Bounding box of the whole polyline, clamped to the image.
    let mut xmin = f64::INFINITY;
    let mut ymin = f64::INFINITY;
    let mut xmax = f64::NEG_INFINITY;
    let mut ymax = f64::NEG_INFINITY;
    for &(x, y) in &points {
        xmin = xmin.min(x);
        ymin = ymin.min(y);
        xmax = xmax.max(x);
        ymax = ymax.max(y);
    }
    let bx0 = (xmin - reach).floor().max(0.0) as u32;
    let by0 = (ymin - reach).floor().max(0.0) as u32;
    let bx1 = (xmax + reach).ceil().min(iw as f64) as u32;
    let by1 = (ymax + reach).ceil().min(ih as f64) as u32;
    if bx0 >= bx1 || by0 >= by1 {
        return; // entirely outside the frame
    }
    let bw = (bx1 - bx0) as usize;
    let bh = (by1 - by0) as usize;
    let mut mask = vec![0u8; bw * bh];

    // Per-segment capsule raster, unioned by max.
    for seg in points.windows(2) {
        let (px, py) = seg[0];
        let (qx, qy) = seg[1];
        let dx = qx - px;
        let dy = qy - py;
        let len2 = dx * dx + dy * dy;
        // Segment-local box, clamped to the stroke bbox.
        let sx0 = ((px.min(qx) - reach).floor().max(bx0 as f64)) as u32;
        let sy0 = ((py.min(qy) - reach).floor().max(by0 as f64)) as u32;
        let sx1 = ((px.max(qx) + reach).ceil().min(bx1 as f64)) as u32;
        let sy1 = ((py.max(qy) + reach).ceil().min(by1 as f64)) as u32;
        for y in sy0..sy1 {
            for x in sx0..sx1 {
                let dxp = x as f64 + 0.5 - px;
                let dyp = y as f64 + 0.5 - py;
                // Distance to the segment (a disc at the endpoint when the
                // segment is degenerate); the endpoint clamp is what makes
                // the round caps for free.
                let d = if len2 == 0.0 {
                    dxp.hypot(dyp)
                } else {
                    let t = ((dxp * dx + dyp * dy) / len2).clamp(0.0, 1.0);
                    (dxp - t * dx).hypot(dyp - t * dy)
                };
                let a = ((radius - d) / EDGE_SOFTNESS_PX).clamp(0.0, 1.0);
                if a <= 0.0 {
                    continue;
                }
                let i = (y - by0) as usize * bw + (x - bx0) as usize;
                let ai = (a * 255.0 + 0.5) as u8;
                // The union of discs is a max, not an over-blend.
                if ai > mask[i] {
                    mask[i] = ai;
                }
            }
        }
    }

    let [cr, cg, cb] = s.color;
    for y in by0..by1 {
        for x in bx0..bx1 {
            let m = mask[(y - by0) as usize * bw + (x - bx0) as usize];
            if m == 0 {
                continue;
            }
            let p = buf.get_pixel(x, y).0;
            let blended = if m == 255 {
                [cr, cg, cb]
            } else {
                let a = m as f64 / 255.0;
                let inva = 1.0 - a;
                [
                    (cr as f64 * a + p[0] as f64 * inva + 0.5) as u8,
                    (cg as f64 * a + p[1] as f64 * inva + 0.5) as u8,
                    (cb as f64 * a + p[2] as f64 * inva + 0.5) as u8,
                ]
            };
            buf.put_pixel(x, y, Rgb(blended));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    fn white(w: u32, h: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, Rgb([255, 255, 255])))
    }

    fn raw(img: &DynamicImage) -> Vec<u8> {
        img.to_rgb8().into_raw()
    }

    fn pixel(img: &DynamicImage, x: u32, y: u32) -> [u8; 3] {
        img.to_rgb8().get_pixel(x, y).0
    }

    fn pen(width: f64, points: &[f64]) -> Stroke {
        Stroke {
            tool: Tool::Pen,
            color: [10, 20, 30],
            width,
            points: points.to_vec(),
        }
    }

    #[test]
    fn empty_strokes_leave_pixels_untouched() {
        let img = white(32, 16);
        let out = apply(&img, &[]);
        assert_eq!(raw(&img), raw(&out), "empty input must be a no-op");
    }

    #[test]
    fn malformed_strokes_are_dropped() {
        let img = white(40, 20);
        // A one-point "stroke" and a NaN point are ignored, not painted.
        let one_point = pen(0.1, &[0.5, 0.5]);
        let nan_point = pen(0.1, &[0.5, 0.5, 0.5, f64::NAN]);
        assert_eq!(raw(&img), raw(&apply(&img, &[one_point])));
        assert_eq!(raw(&img), raw(&apply(&img, &[nan_point])));
    }

    #[test]
    fn pen_stroke_is_an_opaque_band() {
        // 100 x 200 frame: width 0.2 * min side (100) = 20 px thickness.
        // The normalized y 0.5 maps to pixel row 100's centre.
        let img = white(100, 200);
        let out = apply(&img, &[pen(0.2, &[0.5, 0.25, 0.5, 0.75])]);
        assert_eq!(pixel(&out, 50, 100), [10, 20, 30], "centre: opaque ink");
        assert_eq!(pixel(&out, 50, 10), [255, 255, 255], "paper far above");
        assert_eq!(pixel(&out, 25, 100), [255, 255, 255], "paper beside");
    }

    #[test]
    fn whiteout_covers_ink() {
        // Vertical ink stripe on paper; a white-out stroke crosses it.
        let mut img = white(64, 64);
        {
            let buf = img.as_mut_rgb8().unwrap();
            for y in 0..64 {
                buf.put_pixel(32, y, Rgb([40, 40, 40]));
            }
        }
        let paper = Stroke {
            tool: Tool::Whiteout,
            color: [250, 245, 235],
            width: 0.3,
            points: vec![0.1, 0.5, 0.9, 0.5],
        };
        let out = apply(&img, &[paper]);
        assert_eq!(
            pixel(&out, 32, 32),
            [250, 245, 235],
            "white-out is opaque over existing ink at its centre"
        );
        assert_eq!(
            pixel(&out, 10, 10),
            [255, 255, 255],
            "margins away from the stroke are not tinted"
        );
    }

    #[test]
    fn strokes_round_cap_at_the_endpoints() {
        // Endpoints get their own stamped disc, so the line ends in a cap.
        let img = white(100, 40);
        let out = apply(&img, &[pen(0.2, &[0.2, 0.5, 0.8, 0.5])]);
        // The right endpoint is (80, 20); just inside its cap disc.
        assert_eq!(pixel(&out, 81, 20), [10, 20, 30], "cap covers past the end");
        // Well past the cap edge (d > r + softness) is untouched paper.
        assert_eq!(pixel(&out, 88, 20), [255, 255, 255]);
    }

    #[test]
    fn width_is_relative_to_the_min_side() {
        // A wide frame: the same 0.2 width fraction is still 0.2 * height.
        let img = white(200, 100);
        let out = apply(&img, &[pen(0.2, &[0.25, 0.5, 0.75, 0.5])]);
        assert_eq!(pixel(&out, 100, 50), [10, 20, 30]);
        assert_eq!(pixel(&out, 100, 45), [10, 20, 30], "5 px above: inked");
        assert_eq!(pixel(&out, 165, 20), [255, 255, 255], "off the segment");
    }

    #[test]
    fn hairline_on_the_half_pixel_grid() {
        // A sub-half-pixel pen clamps to radius 0.5; a centerline placed
        // exactly on pixel centres still inks those pixels (partially —
        // the alpha ramp has no fully-opaque interior at this radius).
        let img = white(40, 40);
        let out = apply(&img, &[pen(f64::EPSILON, &[0.0, 0.5125, 1.0, 0.5125])]);
        let dot = pixel(&out, 20, 20);
        assert_ne!(dot, [255, 255, 255], "the hairline track shows");
        assert_eq!(pixel(&out, 20, 25), [255, 255, 255], "5 rows away: paper");
    }

    #[test]
    fn points_outside_the_frame_are_clamped() {
        let img = white(40, 40);
        let out = apply(&img, &[pen(0.1, &[-0.5, 0.5, 1.5, 0.5])]);
        // The stamping itself clamps; the visible row is inked across.
        assert_eq!(pixel(&out, 2, 20), [10, 20, 30]);
        assert_eq!(pixel(&out, 37, 20), [10, 20, 30]);
    }

    #[test]
    fn overlapping_segments_union_as_max_not_over() {
        // A stroke whose last segment duplicates the previous one paints the
        // same pixels twice. Coverage must be the union (max), so an edge
        // pixel hit by both keeps the single-disc alpha ramp instead of
        // saturating toward opaque under the (wrong) over operator.
        //
        // Geometry: 200x100 frame, radius 10, line (50,50)-(150,50). Edge
        // pixel (100, 59): its centre is 9.5px off the line, deep in the
        // 1.5px soft ramp — alpha = (10-9.5)/1.5 = 1/3 -> 85/255.
        let img = white(200, 100);
        let once = pen(0.2, &[0.25, 0.5, 0.75, 0.5]);
        let edge = pixel(&apply(&img, &[once]), 100, 59);
        let expected = pen(0.2, &[0.25, 0.5, 0.75, 0.5, 0.75, 0.5]);
        let repainted = pixel(&apply(&img, &[expected]), 100, 59);
        assert_eq!(
            edge, repainted,
            "re-inking the same pixels must not darken them"
        );
        // (10*85 + 255*(255-85))/255 + 0.5 ≈ 173
        assert_eq!(edge, [173, 177, 180], "alpha is the single-disc ramp");
    }

    #[test]
    fn a_fan_of_half_pixel_stamp_lines_does_not_saturate() {
        // The stamp-spacing failure mode Claude flagged: an edge pixel hit
        // by many partial coverages half a pixel apart would saturate toward
        // opaque under the over operator. With the union (max) it keeps the
        // ramp value of its *nearest* capsule.
        //
        // Geometry: 200x100 frame, radius 10, four parallel lines at y = 50,
        // 51, 52, 53 (1px apart). Pixel (100, 62): nearest line is y=53 at
        // d=9.5 -> alpha (10-9.5)/1.5 = 1/3 -> 85/255, exactly like the
        // single-line edge pixel above. Over-blending 4 stamps of a=1/3
        // would land near 200/255 (r≈40); max-union keeps r=173.
        let img = white(200, 100);
        let stroke = pen(
            0.2,
            &[
                0.25, 0.50, 0.75, 0.50, //
                0.25, 0.51, 0.75, 0.51, //
                0.25, 0.52, 0.75, 0.52, //
                0.25, 0.53, 0.75, 0.53,
            ],
        );
        let edge = pixel(&apply(&img, &[stroke]), 100, 62);
        assert_eq!(edge, [173, 177, 180], "ramp value of the nearest capsule");
    }

    #[test]
    #[ignore] // perf check: run with --ignored --nocapture
    fn timing_thick_strokes_on_12mp() {
        // 4000x3000 (12MP), 20 thick pen strokes zig-zagging across the page.
        let img = DynamicImage::new_rgb8(4000, 3000);
        let mut strokes = Vec::new();
        for i in 0..20 {
            let y = (i as f64 + 0.5) / 20.0;
            strokes.push(pen(0.05, &[0.05, y, 0.5, y + 0.2, 0.95, y]));
        }
        strokes.push(Stroke {
            tool: Tool::Whiteout,
            color: [250, 245, 235],
            width: 0.3,
            points: vec![0.1, 0.4, 0.9, 0.6],
        });
        let t = std::time::Instant::now();
        let out = apply(&img, &strokes);
        eprintln!("20 thick strokes over 12MP: {:?}", t.elapsed());
        assert_eq!(out.width(), 4000);
    }
}
