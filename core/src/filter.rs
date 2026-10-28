//! Image filters (grayscale, black & white, contrast).
//!
//! Phone photos of paper have uneven light: shadows, a grey cast, and paper
//! that is darker at one edge than the other. The filters estimate the local
//! paper brightness on a coarse grid, then divide it out. A shadow is lifted
//! to white rather than turning into a dark patch, and ink stays dark because
//! it is compared with the paper around it. Everything is one pass per pixel
//! plus a grid the size of a few hundred cells, so it is fast on 12 MP photos.

use image::{DynamicImage, GrayImage, ImageBuffer, Luma, Rgb, RgbImage};

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
const INK_THRESHOLD: f32 = 0.7;
/// Enhance maps this fraction of the local paper brightness to pure white, so
/// slightly grey paper still comes out white.
const PAPER_WHITE: f32 = 0.9;
/// The background grid has about this many cells across the shorter side.
const GRID_CELLS: u32 = 32;
/// Smallest cell in pixels, so tiny images do not get a grid of one-pixel cells.
const MIN_CELL: u32 = 8;

/// Apply `filter` to a photo and return the result.
///
/// `Grayscale` and `BlackWhite` return `Luma8` images, which the PDF stores
/// compactly. `Enhance` returns `Rgb8`. `Original` returns a copy of the input.
pub fn apply(image: &DynamicImage, filter: Filter) -> DynamicImage {
    match filter {
        Filter::Original => image.clone(),
        Filter::Grayscale => DynamicImage::ImageLuma8(image.to_luma8()),
        Filter::Enhance => DynamicImage::ImageRgb8(enhance(&image.to_rgb8())),
        Filter::BlackWhite => DynamicImage::ImageLuma8(black_white(&image.to_luma8())),
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
/// global threshold would.
fn black_white(gray: &GrayImage) -> GrayImage {
    let (width, height) = gray.dimensions();
    let background = Background::estimate(gray.as_raw(), width, height, 1);
    ImageBuffer::from_fn(width, height, |x, y| {
        let value = gray.get_pixel(x, y).0[0] as f32;
        let paper = background.at(x, y, 0).max(1.0);
        Luma([if value < INK_THRESHOLD * paper {
            0
        } else {
            255
        }])
    })
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
    /// Estimate paper brightness from the brightest pixel in each grid cell.
    /// Ink is darker than paper, so a cell that holds text still reports the
    /// paper. Cells are blended bilinearly, so the estimate changes smoothly
    /// across the page.
    fn estimate(pixels: &[u8], width: u32, height: u32, channels: usize) -> Self {
        let cell = (width.min(height) / GRID_CELLS).max(MIN_CELL);
        let cols = width.div_ceil(cell) as usize;
        let rows = height.div_ceil(cell) as usize;
        let mut values = vec![0u8; cols * rows * channels];

        let stride = width as usize * channels;
        for y in 0..height as usize {
            let row = &pixels[y * stride..(y + 1) * stride];
            let grid_row = (y / cell as usize) * cols;
            for (x, px) in row.chunks_exact(channels).enumerate() {
                let base = (grid_row + x / cell as usize) * channels;
                for (c, &v) in px.iter().enumerate() {
                    values[base + c] = values[base + c].max(v);
                }
            }
        }

        Background {
            cols,
            channels,
            values,
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
