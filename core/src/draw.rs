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

/// Stamp spacing along the path, in pixel space (width only adds more
/// overlapping stamps onto the same coverage mask; spacing stays fine).
const STEP_PX: f64 = 0.5;

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
    // Union coverage for one stroke at a time: stamps blend into the mask
    // with the over operator, so overlapping disc edges accumulate to the
    // true union instead of darkening where they overlap.
    let mut mask = vec![0u8; buf.width() as usize * buf.height() as usize];
    for s in &strokes {
        stamp_stroke(&mut buf, &mut mask, s, w, h);
    }
    DynamicImage::ImageRgb8(buf)
}

/// Stamps one stroke into `buf`, using `mask` as the scratch coverage map
/// (cleared here; only pixels of the stroke's touched box are read back).
fn stamp_stroke(buf: &mut RgbImage, mask: &mut [u8], s: &Stroke, w: f64, h: f64) {
    let (iw, ih) = (buf.width(), buf.height());
    let min_side = w.min(h);
    let radius = (s.width * min_side / 2.0)
        .abs()
        .max(0.5)
        .min(min_side / 4.0);
    let points: Vec<(f64, f64)> = s
        .points
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[x, y]| (x * w, y * h))
        .collect();

    mask[..iw as usize * ih as usize].fill(0);
    let mut touched: Option<(u32, u32, u32, u32)> = None; // x0, y0, x1, y1

    // Walk the polyline, stamping a soft-edged disc every half pixel.
    for seg in points.windows(2) {
        let (px, py) = seg[0];
        let (qx, qy) = seg[1];
        let steps = (((qx - px).hypot(qy - py)) / STEP_PX) as u32 + 1;
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            let (cx, cy) = (px + (qx - px) * t, py + (qy - py) * t);
            // Bounding box of influenced pixels, clamped to the image.
            let x0 = (cx - radius - EDGE_SOFTNESS_PX).floor().max(0.0) as u32;
            let y0 = (cy - radius - EDGE_SOFTNESS_PX).floor().max(0.0) as u32;
            let x1 = (cx + radius + EDGE_SOFTNESS_PX).ceil().min(iw as f64) as u32;
            let y1 = (cy + radius + EDGE_SOFTNESS_PX).ceil().min(ih as f64) as u32;
            for y in y0..y1 {
                for x in x0..x1 {
                    let dx = x as f64 + 0.5 - cx;
                    let dy = y as f64 + 0.5 - cy;
                    let a = ((radius - dx.hypot(dy)) / EDGE_SOFTNESS_PX).clamp(0.0, 1.0);
                    if a <= 0.0 {
                        continue;
                    }
                    let idx = (y * iw + x) as usize;
                    // Accumulate coverage: c' = a + c * (1 - a).
                    let cur = mask[idx] as f64 / 255.0;
                    let merged = a + cur * (1.0 - a);
                    mask[idx] = (merged * 255.0 + 0.5) as u8;
                    touched = Some(match touched {
                        None => (x, y, x, y),
                        Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                    });
                }
            }
        }
    }

    let Some((x0, y0, x1, y1)) = touched else {
        return; // entirely outside the frame
    };
    let [cr, cg, cb] = s.color;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let m = mask[(y * iw + x) as usize];
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
}
