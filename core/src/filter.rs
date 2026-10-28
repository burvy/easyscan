//! Image filters (grayscale, black & white, contrast).
//!
//! Phone photos of paper have uneven light: shadows, a grey cast, and paper
//! that is darker at one edge than the other. The filters estimate the local
//! paper brightness on a coarse grid, then divide it out. A shadow is lifted
//! to white rather than turning into a dark patch, and ink stays dark because
//! it is compared with the paper around it. Everything is one pass per pixel
//! plus a grid the size of a few hundred cells, so it is fast on 12 MP photos.

use image::{DynamicImage, GrayImage, ImageBuffer, Luma, Pixel, Rgb, RgbImage};

/// A scan filter. `Original` returns the photo unchanged, so the UI can always
/// offer a way back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    /// The photo as given, no processing.
    Original,
    /// Plain luminance, one channel.
    Grayscale,
    /// Whitens the paper and keeps colour (coloured ink, highlighter).
    Enhance,
    /// Ink black, paper white, one channel. The smallest output for a PDF.
    BlackWhite,
}

/// A pixel darker than this fraction of the local paper brightness is ink.
/// Faint pencil is only ~15-20% darker than the paper under it, so this
/// has to sit well above where pen ink (~half the paper brightness or
/// less) lives; plain paper texture sits within a few percent of the
/// estimate, so 0.85 leaves a paper-noise margin of the same size the
/// pencil needs.
const INK_THRESHOLD: f32 = 0.85;
/// Notebook rule lines are pale, saturated blue. Cameras under warm light
/// make even their blue channel read darker than red, so "blue" has to be
/// judged against the local paper colour: a pixel whose blue channel is
/// this much closer to paper-bright than its red channel (both relative
/// to the paper estimate) counts as rule-line blue. Pencil stays at the
/// paper's own warmth (~0-0.02 after normalisation), so it is untouched;
/// red pen reads strongly negative.
const RULE_BLUE_EXCESS: f32 = 0.02;
/// Rule-line blue above [`RULE_BLUE_EXCESS`] is only ignored for ink while
/// the pixel is still this bright relative to paper. Blue pen ink (and any
/// readable writing) is far darker than the pale rule lines, so this keeps
/// darker-than-rule blue writing black.
const RULE_MIN_LUMA: f32 = 0.66;
/// Enhance maps this fraction of the local paper brightness to pure white, so
/// slightly grey paper still comes out white.
const PAPER_WHITE: f32 = 0.9;
/// The background grid has about this many cells across the shorter side.
const GRID_CELLS: u32 = 32;
/// Smallest cell in pixels, so tiny images do not get a grid of one-pixel cells.
const MIN_CELL: u32 = 8;
/// The paper brightness of a cell is this quantile of its pixels, not the
/// strict maximum: one hot sensor pixel (or a small bright speck) cannot
/// move the estimate, while paper — the majority of every cell bar those
/// buried in a fat ink stroke — still sets it.
const PAPER_PERCENTILE: f32 = 0.97;
/// A cell fully inside a solid ink area has no paper of its own; its
/// estimate is raised to the brightest cell within this cell radius, so the
/// middle of a solid marker stroke stays black instead of punching a white
/// hole. Solid ink up to 2×radius cells across is covered — about 380 px at
/// 12 MP (a cell is ~94 px), a fat marker blob or filled-in box. Wider
/// blackouts than that still hole out at the centre.
const NEIGHBORHOOD_RADIUS: usize = 2;
/// But the raise only happens for cells that are *buried*: genuinely much
/// darker than the brightest neighbour. A page in a hand shadow is also
/// darker than its neighbours, yet its own estimate is the right one —
/// raising it to the lit paper beyond the shadow edge would turn the
/// shadowed paper black instead of white. A cell measuring less than this
/// fraction of the brightest neighbour is solid ink; any lighter, the
/// shadowed paper keeps its own estimate.
const BURIED_INK_RATIO: f32 = 0.5;

/// Apply `filter` to a photo and return the result.
///
/// `Grayscale` and `BlackWhite` return `Luma8` images, which the PDF stores
/// compactly. `Enhance` returns `Rgb8`. `Original` returns a copy of the input.
pub fn apply(image: &DynamicImage, filter: Filter) -> DynamicImage {
    match filter {
        Filter::Original => image.clone(),
        Filter::Grayscale => DynamicImage::ImageLuma8(image.to_luma8()),
        Filter::Enhance => DynamicImage::ImageRgb8(enhance(&image.to_rgb8())),
        Filter::BlackWhite => DynamicImage::ImageLuma8(black_white(&image.to_rgb8())),
    }
}

/// Whiten the paper and keep colour: each channel is divided by its local
/// paper brightness, so a cream or shadowed page turns white and coloured ink
/// keeps its hue.
fn enhance(rgb: &RgbImage) -> RgbImage {
    let (width, height) = rgb.dimensions();
    let background = Background::estimate(rgb.as_raw(), width, height, 3);
    ImageBuffer::from_fn(width, height, |x, y| {
        let src = rgb.get_pixel(x, y).0;
        let mut out = [0u8; 3];
        for (c, value) in out.iter_mut().enumerate() {
            let paper = background.at(x, y, c).max(1.0) * PAPER_WHITE;
            *value = (src[c] as f32 * 255.0 / paper).min(255.0) as u8;
        }
        Rgb(out)
    })
}

/// Pure black or white. A pixel is ink when it is clearly darker than the
/// paper around it, so a shadow does not blacken a whole region the way one
/// global threshold would. One exception: pale blue notebook rule lines are
/// exactly as dark as faint pencil, so darkness alone cannot keep them out
/// — they are blue instead, and blueness is judged relative to the local
/// paper colour rather than by hue (which the camera's warm white balance
/// would otherwise swamp) — see [`RULE_BLUE_EXCESS`], [`RULE_MIN_LUMA`].
/// Pencil is neutral, red pen strongly red, so only the rule lines drop;
/// blue pen ink stays black thanks to its darkness.
fn black_white(rgb: &RgbImage) -> GrayImage {
    let (width, height) = rgb.dimensions();
    // Per-channel paper estimate — the same grid `enhance` uses — so the
    // blueness test can be relative to what the paper looks like locally.
    let background = Background::estimate(rgb.as_raw(), width, height, 3);
    ImageBuffer::from_fn(width, height, |x, y| {
        let src = rgb.get_pixel(x, y).0;
        let value = Rgb(src).to_luma().0[0] as f32;
        let paper_r = background.at(x, y, 0).max(1.0);
        let paper_b = background.at(x, y, 2).max(1.0);
        let paper_luma =
            0.2126 * paper_r + 0.7152 * background.at(x, y, 1) + 0.0722 * paper_b;
        let ink = value < INK_THRESHOLD * paper_luma.max(1.0)
            && !is_rule_line_blue(src, paper_r, paper_b, value, paper_luma);
        Luma([if ink { 0 } else { 255 }])
    })
}

/// A pale, relatively blue pixel: a notebook rule line (and only the rule
/// line — blue ink is darker, see [`RULE_MIN_LUMA`], and nothing else on a
/// page is blue-tinted relative to the paper it sits on).
fn is_rule_line_blue(
    pixel: [u8; 3],
    paper_r: f32,
    paper_b: f32,
    value: f32,
    paper_luma: f32,
) -> bool {
    let blue_excess = pixel[2] as f32 / paper_b - pixel[0] as f32 / paper_r;
    blue_excess > RULE_BLUE_EXCESS && value > RULE_MIN_LUMA * paper_luma
}

/// Index pair and interpolation weight for one pixel along one axis.
type Weight = (usize, usize, f32);

/// Local paper brightness on a coarse grid, one value per channel per cell.
struct Background {
    cols: usize,
    channels: usize,
    /// `cols * rows * channels` values, row-major, one cell after another.
    values: Vec<u8>,
    /// For each pixel column and row, the two grid cells to blend between.
    xs: Vec<Weight>,
    ys: Vec<Weight>,
}

impl Background {
    /// Estimate paper brightness per grid cell, robust to two failure modes
    /// of the naive per-cell max: a single bright speck inflating the est-
    /// imate (making everything nearby too ink), and a cell buried entirely
    /// in solid ink reporting ink as paper (punching a white hole into it).
    /// Cells are blended bilinearly, so the estimate changes smoothly
    /// across the page.
    fn estimate(pixels: &[u8], width: u32, height: u32, channels: usize) -> Self {
        let cell = (width.min(height) / GRID_CELLS).max(MIN_CELL);
        let cols = width.div_ceil(cell) as usize;
        let rows = height.div_ceil(cell) as usize;
        let cells = cols * rows;

        // Pixel histogram per cell per channel: 256 bins, u32 — one pass,
        // and exact enough to pick a percentile. A 12 MP photo's grid holds
        // about 4 MB of counts for RGB; cheaper than an integral image and
        // plenty for the two questions we ask (see below).
        let mut counts = vec![0u32; cells * channels * 256];
        let mut cell_pixels = vec![0u32; cells];

        let stride = width as usize * channels;
        for y in 0..height as usize {
            let row = &pixels[y * stride..(y + 1) * stride];
            let grid_row = (y / cell as usize) * cols;
            for (x, px) in row.chunks_exact(channels).enumerate() {
                let index = grid_row + x / cell as usize;
                cell_pixels[index] += 1;
                let base = index * channels;
                for (c, &v) in px.iter().enumerate() {
                    counts[(base + c) * 256 + v as usize] += 1;
                }
            }
        }

        // Paper estimate 1: the [`PAPER_PERCENTILE`] quantile of each cell.
        // The strict max was sensitive to one hot pixel; a few pixels cannot
        // move this.
        let mut values = vec![0u8; cells * channels];
        for c_index in 0..cells * channels {
            let target = (cell_pixels[c_index / channels] as f32 * PAPER_PERCENTILE).ceil() as u32;
            let hist = &counts[c_index * 256..(c_index + 1) * 256];
            let mut seen = 0u32;
            for (v, &n) in hist.iter().enumerate() {
                seen += n;
                if seen >= target {
                    values[c_index] = v as u8;
                    break;
                }
            }
            // An empty cell cannot happen (the grid covers every pixel), but
            // if it ever does the value stays 0, which the callers clamp.
        }

        // Paper estimate 2: raise each cell to the brightest neighbour
        // within [`NEIGHBORHOOD_RADIUS`] (measured in cells, so it scales
        // with the page) — but only when the cell is genuinely buried in
        // ink ([`BURIED_INK_RATIO`]); a hand-shadowed page also measures
        // darker than its neighbours, and its own estimate is the right
        // one there. For normal lit cells this is a no-op, given how
        // gently lighting changes over that distance.
        let mut raised = vec![0u8; cells * channels];
        for j in 0..rows {
            for i in 0..cols {
                for c in 0..channels {
                    let own = values[(j * cols + i) * channels + c];
                    let mut m = own;
                    for jj in j.saturating_sub(NEIGHBORHOOD_RADIUS)
                        ..=(j + NEIGHBORHOOD_RADIUS).min(rows - 1)
                    {
                        for ii in i.saturating_sub(NEIGHBORHOOD_RADIUS)
                            ..=(i + NEIGHBORHOOD_RADIUS).min(cols - 1)
                        {
                            m = m.max(values[(jj * cols + ii) * channels + c]);
                        }
                    }
                    raised[(j * cols + i) * channels + c] =
                        if (own as f32) < BURIED_INK_RATIO * m as f32 {
                            m
                        } else {
                            own
                        };
                }
            }
        }

        Background {
            cols,
            channels,
            values: raised,
            xs: axis_weights(width, cell, cols),
            ys: axis_weights(height, cell, rows),
        }
    }

    /// Interpolated paper brightness at pixel `(x, y)` for channel `c`.
    fn at(&self, x: u32, y: u32, c: usize) -> f32 {
        let (x0, x1, tx) = self.xs[x as usize];
        let (y0, y1, ty) = self.ys[y as usize];
        let value =
            |i: usize, j: usize| self.values[(j * self.cols + i) * self.channels + c] as f32;
        let top = value(x0, y0) + (value(x1, y0) - value(x0, y0)) * tx;
        let bottom = value(x0, y1) + (value(x1, y1) - value(x0, y1)) * tx;
        top + (bottom - top) * ty
    }
}

/// For each pixel position along one axis, the two nearest cell centres and
/// the blend weight between them. Cell `i` is centred on pixel
/// `i * cell + cell / 2`; positions past the outermost centres clamp to them.
fn axis_weights(len: u32, cell: u32, cells: usize) -> Vec<Weight> {
    let last = (cells - 1) as f32;
    (0..len)
        .map(|p| {
            let g = ((p as f32 + 0.5) / cell as f32 - 0.5).clamp(0.0, last);
            let i0 = g.floor() as usize;
            let i1 = (i0 + 1).min(cells - 1);
            (i0, i1, g - i0 as f32)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// Ink strokes: two vertical bars and one horizontal bar.
    fn is_ink(x: u32, y: u32) -> bool {
        (30..34).contains(&x) || (170..174).contains(&x) || (60..64).contains(&y)
    }

    /// Paper that is bright on the left and in shadow on the right.
    fn paper_at(x: u32, width: u32) -> f32 {
        240.0 - 130.0 * x as f32 / (width - 1) as f32
    }

    fn shadowed_page(width: u32, height: u32) -> DynamicImage {
        let gray: GrayImage = ImageBuffer::from_fn(width, height, |x, y| {
            Luma([if is_ink(x, y) {
                20
            } else {
                paper_at(x, width) as u8
            }])
        });
        DynamicImage::ImageLuma8(gray)
    }

    #[test]
    fn original_is_identity() {
        let rgb: RgbImage = ImageBuffer::from_fn(17, 9, |x, y| Rgb([x as u8, y as u8, 99]));
        let input = DynamicImage::ImageRgb8(rgb.clone());
        match apply(&input, Filter::Original) {
            DynamicImage::ImageRgb8(out) => assert_eq!(out, rgb),
            other => panic!("expected Rgb8, got {:?}", other.color()),
        }
    }

    #[test]
    fn grayscale_is_luma8_and_keeps_grey_levels() {
        let grey: RgbImage = ImageBuffer::from_pixel(8, 8, Rgb([128, 128, 128]));
        let out = apply(&DynamicImage::ImageRgb8(grey), Filter::Grayscale);
        match out {
            DynamicImage::ImageLuma8(img) => assert!(img.pixels().all(|p| p.0[0] == 128)),
            other => panic!("expected Luma8, got {:?}", other.color()),
        }
    }

    #[test]
    fn black_white_keeps_ink_and_whitens_both_sides_of_shadow() {
        let (width, height) = (200, 100);
        let input = shadowed_page(width, height);
        let out = match apply(&input, Filter::BlackWhite) {
            DynamicImage::ImageLuma8(img) => img,
            other => panic!("expected Luma8, got {:?}", other.color()),
        };

        for y in 0..height {
            for x in 0..width {
                let expected = if is_ink(x, y) { 0 } else { 255 };
                assert_eq!(out.get_pixel(x, y).0[0], expected, "pixel ({x}, {y})");
            }
        }
    }

    #[test]
    fn enhance_whitens_paper_and_keeps_ink_colour() {
        let (width, height) = (200u32, 100u32);
        // Cream paper, darker to the right; red ink, darker in the shadow too.
        let rgb: RgbImage = ImageBuffer::from_fn(width, height, |x, y| {
            let shade = 1.0 - 0.5 * x as f32 / (width - 1) as f32;
            let base = if is_ink(x, y) {
                [150.0, 20.0, 20.0]
            } else {
                [240.0, 230.0, 200.0]
            };
            Rgb(base.map(|v| (v * shade) as u8))
        });
        let out = match apply(&DynamicImage::ImageRgb8(rgb), Filter::Enhance) {
            DynamicImage::ImageRgb8(img) => img,
            other => panic!("expected Rgb8, got {:?}", other.color()),
        };

        // Lit side and shadowed side: paper goes white, ink stays red.
        for (paper_x, ink_x) in [(10u32, 31u32), (190, 172)] {
            let paper = out.get_pixel(paper_x, 10).0;
            assert!(
                paper.iter().all(|&v| v >= 250),
                "paper at x={paper_x} is {paper:?}"
            );
            let ink = out.get_pixel(ink_x, 10).0;
            assert!(
                ink[0] > 150 && ink[1] < 60 && ink[2] < 60,
                "ink at x={ink_x} is {ink:?}"
            );
        }
    }

    /// A single bright speck must not inflate the paper estimate of its
    /// cell: pixels under the old strict-max estimate (which turns the
    /// speck's cell to "paper 255") used to speckle black. Large enough to
    /// make the grid cells realistic (~30 px at this size).
    #[test]
    fn bright_speck_does_not_speckle_the_paper() {
        let speck = (599u32, 300); // centre cell of a 1200×600 page
        let smudge = (560..640, 260..340); // grey paper, same cell area
        let gray: GrayImage = ImageBuffer::from_fn(1200, 600, |x, y| {
            let v = if smudge.0.contains(&x) && smudge.1.contains(&y) {
                170.0
            } else {
                200.0
            };
            // 4 speck pixels, bright: under a strict-max estimate these 4
            // pixels alone ruled a ~900-px cell.
            Luma([
                if (speck.0..speck.0 + 2).contains(&x) && (speck.1..speck.1 + 2).contains(&y) {
                    255
                } else {
                    v as u8
                },
            ])
        });
        let out = match apply(&DynamicImage::ImageLuma8(gray), Filter::BlackWhite) {
            DynamicImage::ImageLuma8(img) => img,
            other => panic!("expected Luma8, got {:?}", other.color()),
        };

        // Grey paper and speck: no black speckle. The smudge is the paper
        // itself (dirty but not ink), and the speck is not ink either.
        for y in smudge.1.clone() {
            for x in smudge.0.clone() {
                let v = out.get_pixel(x, y).0[0];
                assert_eq!(v, 255, "speckle at ({x}, {y})");
            }
        }
    }

    /// Rule lines, pencil, and blue pen all sit at nearly the same
    /// darkness in a warm-lit photo (real photo measurements: rule line
    /// rgb(162,149,146) on paper rgb(204,185,173), pencil rgb(148,132,128)).
    /// Only the rule line must disappear; pencil (neutral warmth) and blue
    /// pen (far darker) stay black.
    #[test]
    fn blue_rule_line_vanishes_but_pencil_and_blue_pen_stay() {
        const PAPER: [u8; 3] = [204, 185, 173];
        const RULE: [u8; 3] = [162, 149, 146];
        const PENCIL: [u8; 3] = [148, 132, 128];
        const BLUE_PEN: [u8; 3] = [60, 80, 170];
        const RED_PEN: [u8; 3] = [150, 40, 40];

        let rgb: RgbImage = ImageBuffer::from_fn(1200, 600, |x, y| {
            let colour = if y == 300 {
                RULE
            } else if (500..516).contains(&x) {
                BLUE_PEN
            } else if (700..716).contains(&x) {
                PENCIL
            } else if (400..416).contains(&x) {
                RED_PEN
            } else {
                PAPER
            };
            Rgb(colour)
        });
        let out = match apply(&DynamicImage::ImageRgb8(rgb), Filter::BlackWhite) {
            DynamicImage::ImageLuma8(img) => img,
            other => panic!("expected Luma8, got {:?}", other.color()),
        };

        // Rule line is gone.
        for x in (600..700).step_by(4) {
            assert_eq!(out.get_pixel(x, 300).0[0], 255, "rule line at ({x}, 300)");
        }
        // Pencil, blue pen, red pen stay legible.
        for y in (100..200).step_by(7) {
            assert_eq!(out.get_pixel(708, y).0[0], 0, "pencil at (708, {y})");
            assert_eq!(out.get_pixel(508, y).0[0], 0, "blue pen at (508, {y})");
            assert_eq!(out.get_pixel(408, y).0[0], 0, "red pen at (408, {y})");
        }
    }

    /// A solid ink patch bigger than a grid cell: cells with no paper of
    /// their own must borrow from the neighbourhood instead of classifying
    /// their ink as "local paper" (which used to punch a white hole into
    /// the middle of, e.g., a fat marker block).
    #[test]
    fn solid_ink_patch_stays_black_in_the_middle() {
        let patch = (500u32..568, 250..318); // ~3.8×3.8 cells of solid ink
        let gray: GrayImage = ImageBuffer::from_fn(1200, 600, |x, y| {
            Luma([if patch.0.contains(&x) && patch.1.contains(&y) {
                30
            } else {
                230
            }])
        });
        let out = match apply(&DynamicImage::ImageLuma8(gray), Filter::BlackWhite) {
            DynamicImage::ImageLuma8(img) => img,
            other => panic!("expected Luma8, got {:?}", other.color()),
        };

        for y in patch.1.clone() {
            for x in patch.0.clone() {
                let v = out.get_pixel(x, y).0[0];
                assert_eq!(v, 0, "white hole at ({x}, {y})");
            }
        }
    }

    /// Hand-run visual harness over the private real photos in
    /// `testdata/photos/` (gitignored — never commit them or their
    /// outputs): `cargo test -p easyscan-core --release -- --ignored
    /// --nocapture photo_filter_preview`
    ///
    /// Decodes each photo with its EXIF orientation applied, crops it with
    /// today's page detection (full frame when detection fails), runs every
    /// filter, and writes `target/filter_preview/<photo>/<filter>.png` for
    /// side-by-side eyeballing.
    #[test]
    #[ignore]
    fn photo_filter_preview() {
        use image::ImageDecoder;

        let photos = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/photos");
        let out_root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/filter_preview");
        let _ = std::fs::remove_dir_all(&out_root);

        let Ok(entries) = std::fs::read_dir(&photos) else {
            eprintln!("no photos dir at {}; nothing to preview", photos.display());
            return;
        };
        let mut paths: Vec<std::path::PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("jpg"))
            })
            .collect();
        paths.sort();
        if paths.is_empty() {
            eprintln!("no photos in {}; nothing to preview", photos.display());
            return;
        }
        let photo_count = paths.len();

        let start = Instant::now();
        for path in paths {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("photo")
                .to_string();

            // Orient + decode exactly like the bridge does for camera JPEGs.
            let reader = image::ImageReader::open(&path).expect("open photo");
            let mut decoder = reader.into_decoder().expect("photo decoder");
            let orientation = decoder.orientation().expect("photo orientation");
            let mut photo = image::DynamicImage::from_decoder(decoder).expect("photo decode");
            photo.apply_orientation(orientation);

            // The page the app would process, or the full frame if the
            // detector fails on this photo.
            let detection = crate::detect::detect_page(&photo);
            let page = if detection.found {
                crate::detect::warp(&photo, &detection.quad)
            } else {
                eprintln!("{stem}: detection miss, filtering the full frame");
                photo.clone()
            };

            let out_dir = out_root.join(&stem);
            std::fs::create_dir_all(&out_dir).expect("preview dir");
            for filter in [
                Filter::Original,
                Filter::Grayscale,
                Filter::Enhance,
                Filter::BlackWhite,
            ] {
                let name = format!("{filter:?}.png").to_lowercase();
                apply(&page, filter)
                    .save(out_dir.join(&name))
                    .expect("save preview");
            }
            eprintln!("{stem}: page {}×{} -> 4 PNGs", page.width(), page.height());
        }
        eprintln!(
            "photo preview: {photo_count} photos in {:?}",
            start.elapsed()
        );
    }

    /// Timing check on a 12 MP photo. Run with
    /// `cargo test --release -- --ignored --nocapture black_white_12mp`.
    #[test]
    #[ignore]
    fn black_white_12mp_is_fast() {
        let (width, height) = (4000u32, 3000u32);
        let photo: RgbImage = ImageBuffer::from_fn(width, height, |x, y| {
            let shade = 230.0 - 90.0 * x as f32 / width as f32;
            let v = if is_ink(x, y) { 20.0 } else { shade };
            Rgb([v as u8, v as u8, v as u8])
        });
        let input = DynamicImage::ImageRgb8(photo);

        let start = Instant::now();
        let out = apply(&input, Filter::BlackWhite);
        let elapsed = start.elapsed();
        eprintln!("12 MP black & white: {elapsed:?}");

        assert!(matches!(out, DynamicImage::ImageLuma8(_)));
        if !cfg!(debug_assertions) {
            assert!(elapsed.as_secs_f32() < 1.0, "took {elapsed:?}");
        }
    }
}
