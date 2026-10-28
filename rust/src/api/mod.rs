//! Thin bridge over `easyscan-core`: decode/encode at the edges, no logic.
//!
//! File-path API only: camera JPEG paths in, processed JPEG/PDF files out,
//! EXIF orientation applied at every decode edge. (The byte-based task 007
//! API that round-tripped 12MP PNGs through FFI was removed in task 012.)

use easyscan_core::{detect, draw, filter, pdf};
use image::codecs::jpeg::JpegEncoder;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader};

#[flutter_rust_bridge::frb(init)]
pub fn init_app() {
    flutter_rust_bridge::setup_default_user_utils();
}

/// Version of the Rust core, to prove the bridge is wired up.
#[flutter_rust_bridge::frb(sync)]
pub fn core_version() -> String {
    format!("easyscan-core {}", env!("CARGO_PKG_VERSION"))
}

/// A corner in pixel coordinates (any coordinate frame is fine; callers
/// document which one). Not part of the old byte API.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Corner {
    pub x: f32,
    pub y: f32,
}

pub struct PageDetection {
    /// Top-left, top-right, bottom-right, bottom-left, in source pixels.
    pub corners: Vec<Corner>,
    pub found: bool,
    pub confidence: f32,
}
pub enum ScanFilter {
    Original,
    Grayscale,
    Enhance,
    BlackWhite,
}

pub enum PdfPageSize {
    /// Page = image size at the export dpi.
    FitToImage,
    /// A4, landscape when the image is wider than it is tall.
    A4,
    /// US Letter, landscape when the image is wider than it is tall.
    Letter,
}

/// Oriented width/height in pixels.
pub struct ImageSize {
    pub width: u32,
    pub height: u32,
}

/// One annotation stroke over the page.
///
/// `points` are flat x, y pairs normalized to [0..1]² of the warped result
/// frame (the corners' frame); `width` is the line thickness as a fraction
/// of the frame's short side. `tool`: 0 = pen, 1 = white-out. `color` packs
/// opaque ARGB (the alpha byte is ignored; strokes draw fully opaque).
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeSpec {
    pub tool: u32,
    pub color: u32,
    pub width: f64,
    pub points: Vec<f64>,
}

/// ARGB int -> opaque sRGB, with unknown tools rejected (None = skip).
fn to_stroke(spec: StrokeSpec) -> Option<draw::Stroke> {
    let tool = match spec.tool {
        0 => draw::Tool::Pen,
        1 => draw::Tool::Whiteout,
        _ => return None,
    };
    let [_, r, g, b] = spec.color.to_be_bytes();
    Some(draw::Stroke {
        tool,
        color: [r, g, b],
        width: spec.width,
        points: spec.points,
    })
}

// ---------------------------------------------------------------------------
// File-path API: camera JPEG paths in, processed JPEG/PDF files out.
// ---------------------------------------------------------------------------

/// Decode the image at `path` with its EXIF orientation applied, so photos
/// taken sideways land upright in the pipeline.
fn load_oriented(path: &str) -> Result<DynamicImage, String> {
    let reader = ImageReader::open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut decoder = reader.into_decoder().map_err(|e| format!("{path}: {e}"))?;
    let orientation = decoder.orientation().map_err(|e| format!("{path}: {e}"))?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| format!("{path}: {e}"))?;
    img.apply_orientation(orientation);
    Ok(img)
}

fn to_filter(filter: ScanFilter) -> filter::Filter {
    match filter {
        ScanFilter::Original => filter::Filter::Original,
        ScanFilter::Grayscale => filter::Filter::Grayscale,
        ScanFilter::Enhance => filter::Filter::Enhance,
        ScanFilter::BlackWhite => filter::Filter::BlackWhite,
    }
}

fn to_page_size(page_size: PdfPageSize) -> pdf::PageSize {
    match page_size {
        PdfPageSize::FitToImage => pdf::PageSize::FitToImage,
        PdfPageSize::A4 => pdf::PageSize::A4,
        PdfPageSize::Letter => pdf::PageSize::Letter,
    }
}

/// `rotation_quarters` clockwise quarter-turns on top of the EXIF orientation.
fn rotate_quarters(img: DynamicImage, quarters: u32) -> DynamicImage {
    match quarters % 4 {
        0 => img,
        1 => img.rotate90(),
        2 => img.rotate180(),
        3 => img.rotate270(),
        _ => unreachable!(),
    }
}

fn encode_jpeg_img(img: &DynamicImage, quality: u32) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100) as u8)
        .encode_image(img)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Oriented apparent size of the image file, without decoding the pixels.
pub fn image_size_file(path: String) -> Result<ImageSize, String> {
    let reader = ImageReader::open(&path).map_err(|e| format!("{path}: {e}"))?;
    let mut decoder = reader.into_decoder().map_err(|e| format!("{path}: {e}"))?;
    let (w, h) = decoder.dimensions();
    // Only quarter-turn orientations swap the axes; flips keep them.
    let (width, height) = match decoder.orientation().map_err(|e| format!("{path}: {e}"))? {
        Orientation::Rotate90
        | Orientation::Rotate270
        | Orientation::Rotate90FlipH
        | Orientation::Rotate270FlipH => (h, w),
        _ => (w, h),
    };
    Ok(ImageSize { width, height })
}

/// Detects the page quad in the JPEG at `src_path`.
///
/// The image is first EXIF-oriented, then given `rotation_quarters` extra
/// clockwise quarter-turns; the returned corners are pixel coordinates in
/// that same frame, so `detect_page_file(src, r).corners` are valid input
/// for `process_page_file(src, .., r, ..)`.
pub fn detect_page_file(src_path: String, rotation_quarters: u32) -> Result<PageDetection, String> {
    let img = load_oriented(&src_path)?;
    let img = rotate_quarters(img, rotation_quarters);
    let d = detect::detect_page(&img);
    Ok(PageDetection {
        corners: d.quad.0.iter().map(|p| Corner { x: p.x, y: p.y }).collect(),
        found: d.found,
        confidence: d.confidence,
    })
}

/// One page file in, one processed JPEG file out (UI previews and full-res
/// exports both go through this).
///
/// Order of operations: EXIF orientation, `rotation_quarters` clockwise
/// quarter-turns, perspective warp with `corners` (TL, TR, BR, BL in the
/// oriented *and* rotated frame), annotation `strokes` stamped in the
/// warped frame, `filter`, downscale to fit `max_long_side` (aspect kept;
/// `None` = full resolution), JPEG encode at `jpeg_quality`, write to
/// `out_path`. Returns the written size. No strokes = byte-for-byte the
/// same page as today.
///
/// The parallel page inputs make this the one exception to the 7-argument
/// comfort limit; the Dart side always calls it with named arguments.
#[allow(clippy::too_many_arguments)]
pub fn process_page_file(
    src_path: String,
    corners: Vec<Corner>,
    filter: ScanFilter,
    rotation_quarters: u32,
    strokes: Option<Vec<StrokeSpec>>,
    out_path: String,
    max_long_side: Option<u32>,
    jpeg_quality: u32,
) -> Result<ImageSize, String> {
    let [a, b, c, d]: [Corner; 4] = corners
        .try_into()
        .map_err(|_| "need 4 corners".to_string())?;
    let pt = |p: Corner| detect::Point { x: p.x, y: p.y };
    let quad = detect::Quad([pt(a), pt(b), pt(c), pt(d)]);
    let strokes: Vec<draw::Stroke> = strokes
        .unwrap_or_default()
        .drain(..)
        .filter_map(to_stroke)
        .collect();

    let img = load_oriented(&src_path)?;
    let img = rotate_quarters(img, rotation_quarters);
    let warped = detect::warp(&img, &quad);
    // Strokes live in the warp's output frame and are stamped before the
    // filter, so the filter decides how they render (pen ink binarizes,
    // white-out binarizes back to paper). Empty = exactly today's bytes.
    let warped = if strokes.is_empty() {
        warped
    } else {
        draw::apply(&warped, &strokes)
    };
    let filtered = easyscan_core::filter::apply(&warped, to_filter(filter));
    let sized = match max_long_side {
        Some(limit) if filtered.width().max(filtered.height()) > limit => {
            filtered.thumbnail(limit, limit)
        }
        _ => filtered,
    };
    let jpeg = encode_jpeg_img(&sized, jpeg_quality)?;
    std::fs::write(&out_path, jpeg).map_err(|e| format!("{out_path}: {e}"))?;
    Ok(ImageSize {
        width: sized.width(),
        height: sized.height(),
    })
}

/// One PDF from ordered page-image paths, written to `out_path`.
///
/// Fast path: a page that is already an embeddable baseline JPEG (like
/// everything `process_page_file` writes — no EXIF) is embedded
/// byte-for-byte, so a 20-page export decodes nothing. Otherwise the page
/// is decoded (EXIF orientation applied), re-encoded, and embedded — one
/// page at a time either way, so peak memory is about one bitmap, not one
/// per page.
pub fn export_pdf_file(
    pages: Vec<String>,
    out_path: String,
    page_size: PdfPageSize,
    jpeg_quality: u32,
) -> Result<(), String> {
    if pages.is_empty() {
        return Err("no pages".into());
    }
    let inputs = pages
        .iter()
        .map(|p| -> Result<pdf::PageInput, String> {
            // One file in memory at a time; the compressed bytes drop as
            // soon as the page takes them.
            let bytes = std::fs::read(p).map_err(|e| format!("{p}: {e}"))?;
            match pdf::Jpeg::new(bytes) {
                Ok(jpeg) => Ok(pdf::PageInput::Jpeg(jpeg)),
                Err(reason) => load_oriented(p)
                    .map(pdf::PageInput::Image)
                    .map_err(|e| format!("{p}: {reason}; fallback failed: {e}")),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let options = pdf::PdfOptions {
        jpeg_quality: jpeg_quality.clamp(1, 100) as u8,
        page_size: to_page_size(page_size),
        ..pdf::PdfOptions::default()
    };
    let file = std::fs::File::create(&out_path).map_err(|e| format!("{out_path}: {e}"))?;
    pdf::export_pdf_pages(
        inputs.into_iter(),
        &options,
        &mut std::io::BufWriter::new(file),
    )
    .map_err(|e| format!("{out_path}: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Instant;

    use image::{Rgb, RgbImage};

    /// A JPEG bytes + EXIF orientation value -> JPEG bytes with that EXIF
    /// APP1 segment (TIFF header + one Orientation short) injected right
    /// after the SOI marker.
    fn jpeg_with_exif_orientation(jpeg: &[u8], orientation: u16) -> Vec<u8> {
        assert!(jpeg.starts_with(&[0xFF, 0xD8]), "need a clean JPEG");
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"MM\x00\x2A"); // big-endian TIFF
        tiff.extend_from_slice(&[0, 0, 0, 8]); // IFD0 follows the header
        tiff.extend_from_slice(&[0, 1]); // one entry
        // Entry: tag 0x0112 (Orientation), type 3 (SHORT), count 1, value
        // big-endian in the first two of the four inline value bytes.
        tiff.extend_from_slice(&[
            0x01,
            0x12, // tag
            0x00,
            0x03, // type SHORT
            0,
            0,
            0,
            1, // count
            (orientation >> 8) as u8,
            (orientation & 0xFF) as u8,
            0,
            0, // inline value, zero-padded
        ]);
        tiff.extend_from_slice(&[0, 0, 0, 0]); // no next IFD
        let mut app1 = Vec::new();
        app1.extend_from_slice(b"Exif\x00\x00");
        app1.extend_from_slice(&tiff);

        let mut out = vec![0xFF, 0xD8];
        out.extend_from_slice(&[0xFF, 0xE1]);
        // The APP1 length counts its own two length bytes.
        out.extend_from_slice(&((app1.len() as u16 + 2).to_be_bytes()));
        out.extend_from_slice(&app1);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    /// A stored 40×20 RGB JPEG whose lower half is a bright page on a dark
    /// surface — with EXIF orientation 6 the app must see a 20×40 portrait
    /// with the page along its left side.
    fn sideways_page_jpeg(orientation: u16) -> Vec<u8> {
        let mut img = image::RgbImage::from_fn(40, 20, |x, y| {
            let page = y > 11 && (3..=36).contains(&x);
            let dark = (((x * 7 + y * 13) % 60) + 40) as u8;
            if page {
                image::Rgb([250, 250, 250])
            } else {
                image::Rgb([dark, dark, dark])
            }
        });
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 90)
            .encode_image(&DynamicImage::ImageRgb8(std::mem::take(&mut img)))
            .unwrap();
        jpeg_with_exif_orientation(&jpeg, orientation)
    }

    fn temp_file(name: &str, bytes: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn exif_orientation_6_is_applied() {
        let path = temp_file("easyscan_rot6.jpg", &sideways_page_jpeg(6));

        let size = image_size_file(path.display().to_string()).unwrap();
        assert_eq!(
            (20, 40),
            (size.width, size.height),
            "orientation must swap the axes"
        );

        let detected = detect_page_file(path.display().to_string(), 0).unwrap();
        assert!(detected.found, "the page should be détectable");
        // The page sits along the LEFT of the oriented frame; if the EXIF
        // rotation had been ignored the bright band would be at the bottom.
        assert!(
            detected
                .corners
                .iter()
                .all(|c| c.x >= 0.0 && c.x <= 9.0 && c.y >= 0.0 && c.y <= 40.0),
            "corners must be in the oriented frame: {:?}",
            detected.corners
        );
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn process_page_writes_upright_jpeg() {
        let src = temp_file("easyscan_rot6b.jpg", &sideways_page_jpeg(6));
        let out = std::env::temp_dir().join("easyscan_rot6b_out.jpg");
        let size = process_page_file(
            src.display().to_string(),
            vec![
                Corner { x: 0.0, y: 0.0 },
                Corner { x: 20.0, y: 0.0 },
                Corner { x: 20.0, y: 40.0 },
                Corner { x: 0.0, y: 40.0 },
            ],
            ScanFilter::Original,
            0,
            None,
            out.display().to_string(),
            None,
            80,
        )
        .unwrap();
        assert_eq!(
            (20, 40),
            (size.width, size.height),
            "an upright page, not the sideways 40×20"
        );
        let written = image::open(&out).unwrap();
        assert_eq!((20, 40), (written.width(), written.height()));
        std::fs::remove_file(&src).unwrap();
        std::fs::remove_file(&out).unwrap();
    }

    #[test]
    fn process_page_downscales_and_rotates() {
        let src = temp_file("easyscan_plain.jpg", &{
            let img = image::RgbImage::from_fn(120, 60, |x, y| {
                image::Rgb([(x % 255) as u8, (y % 255) as u8, 128])
            });
            let mut jpeg = Vec::new();
            JpegEncoder::new_with_quality(&mut jpeg, 90)
                .encode_image(&DynamicImage::ImageRgb8(img))
                .unwrap();
            jpeg
        });
        let out = std::env::temp_dir().join("easyscan_down_out.jpg");
        let size = process_page_file(
            src.display().to_string(),
            // Corners live in the rotated frame: one clockwise turn of the
            // 120×60 source becomes 60×120, so the frame is 59×119.
            vec![
                Corner { x: 0.0, y: 0.0 },
                Corner { x: 59.0, y: 0.0 },
                Corner { x: 59.0, y: 119.0 },
                Corner { x: 0.0, y: 119.0 },
            ],
            ScanFilter::Grayscale,
            1, // one clockwise quarter-turn of the landscape source
            None,
            out.display().to_string(),
            Some(40),
            75,
        )
        .unwrap();
        assert_eq!(
            (20, 40),
            (size.width, size.height),
            "rotated then fitted to a 40-long-side limit"
        );
        std::fs::remove_file(&src).unwrap();
        std::fs::remove_file(&out).unwrap();
    }

    /// A process call with no strokes (neither `None` nor an empty list)
    /// must produce exactly the page bytes that the pre-strokes pipeline
    /// produces, so unannotated pages keep their byte-level stability.
    #[test]
    fn empty_strokes_are_byte_identical_to_no_strokes() {
        let src = temp_file("easyscan_draw_plain.jpg", &{
            let img = RgbImage::from_fn(120, 60, |x, y| {
                Rgb([((x * 11 + y * 3) % 200 + 40) as u8, (y % 255) as u8, 128])
            });
            let mut jpeg = Vec::new();
            JpegEncoder::new_with_quality(&mut jpeg, 90)
                .encode_image(&DynamicImage::ImageRgb8(img))
                .unwrap();
            jpeg
        });
        let corners = vec![
            Corner { x: 0.0, y: 0.0 },
            Corner { x: 119.0, y: 0.0 },
            Corner { x: 119.0, y: 59.0 },
            Corner { x: 0.0, y: 59.0 },
        ];
        let run = |strokes: Option<Vec<StrokeSpec>>, name: &str| {
            let out = std::env::temp_dir().join(name);
            process_page_file(
                src.display().to_string(),
                corners.clone(),
                ScanFilter::Enhance,
                0,
                strokes,
                out.display().to_string(),
                None,
                90,
            )
            .unwrap();
            std::fs::read(&out).unwrap()
        };
        let none = run(None, "easyscan_draw_none.jpg");
        let empty = run(Some(vec![]), "easyscan_draw_empty.jpg");
        assert_eq!(none, empty, "an empty stroke list changes nothing");
        std::fs::remove_file(&src).unwrap();
        std::fs::remove_file(&std::env::temp_dir().join("easyscan_draw_none.jpg")).unwrap();
        std::fs::remove_file(&std::env::temp_dir().join("easyscan_draw_empty.jpg")).unwrap();
    }

    #[test]
    fn strokes_burn_onto_the_rendered_page() {
        let src = temp_file("easyscan_draw_src.jpg", &{
            let img = RgbImage::from_pixel(120, 60, Rgb([240, 240, 240]));
            let mut jpeg = Vec::new();
            JpegEncoder::new_with_quality(&mut jpeg, 95)
                .encode_image(&DynamicImage::ImageRgb8(img))
                .unwrap();
            jpeg
        });
        let out = std::env::temp_dir().join("easyscan_draw_burned.jpg");
        let size = process_page_file(
            src.display().to_string(),
            vec![
                Corner { x: 0.0, y: 0.0 },
                Corner { x: 119.0, y: 0.0 },
                Corner { x: 119.0, y: 59.0 },
                Corner { x: 0.0, y: 59.0 },
            ],
            ScanFilter::Original,
            0,
            Some(vec![StrokeSpec {
                tool: 0,           // pen
                color: 0xFF141414, // near-black
                width: 0.2,
                points: vec![0.25, 0.5, 0.75, 0.5],
            }]),
            out.display().to_string(),
            None,
            90,
        )
        .unwrap();
        // The warp output spans the corners: 119 x 59, not 120 x 60.
        assert_eq!((119, 59), (size.width, size.height));
        let rendered = image::open(&out).unwrap();
        // The result frame's centre must be inked (JPEG round trip keeps
        // an opaque dark band well away from the pale paper).
        let px = rendered.to_rgb8().get_pixel(60, 30).0;
        assert!(
            px[0] < 100 && px[1] < 100 && px[2] < 100,
            "the pen stroke must be burned in: {px:?}"
        );
        // And the same page without strokes stays pale paper.
        let plain = std::env::temp_dir().join("easyscan_draw_burnplain.jpg");
        process_page_file(
            src.display().to_string(),
            vec![
                Corner { x: 0.0, y: 0.0 },
                Corner { x: 119.0, y: 0.0 },
                Corner { x: 119.0, y: 59.0 },
                Corner { x: 0.0, y: 59.0 },
            ],
            ScanFilter::Original,
            0,
            None,
            plain.display().to_string(),
            None,
            90,
        )
        .unwrap();
        let px = image::open(&plain).unwrap().to_rgb8().get_pixel(60, 30).0;
        assert!(
            px[0] > 200,
            "unannotated page stays paper at the centre: {px:?}"
        );
        std::fs::remove_file(&src).unwrap();
        std::fs::remove_file(&out).unwrap();
        std::fs::remove_file(&plain).unwrap();
    }

    #[test]
    fn export_pdf_file_writes_a_real_pdf() {
        let src1 = temp_file("easyscan_pdf1.jpg", &sideways_page_jpeg(6));
        let src2 = temp_file("easyscan_pdf2.jpg", &{
            let img = image::RgbImage::from_fn(80, 60, |x, y| {
                image::Rgb([(x + y) as u8, (x.max(y)) as u8, 200])
            });
            let mut jpeg = Vec::new();
            JpegEncoder::new_with_quality(&mut jpeg, 90)
                .encode_image(&image::DynamicImage::ImageRgb8(img))
                .unwrap();
            jpeg
        });
        let out = std::env::temp_dir().join("easyscan_export.pdf");
        export_pdf_file(
            vec![src1.display().to_string(), src2.display().to_string()],
            out.display().to_string(),
            PdfPageSize::FitToImage,
            80,
        )
        .unwrap();
        let bytes = std::fs::read(&out).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
        assert!(bytes.ends_with(b"%%EOF\n"));
        let s = String::from_utf8_lossy(&bytes);
        assert_eq!(2, s.match_indices("/Type /Page ").count());
        std::fs::remove_file(&src1).unwrap();
        std::fs::remove_file(&src2).unwrap();
        std::fs::remove_file(&out).unwrap();
    }

    #[test]
    fn export_pdf_file_embeds_processed_jpegs_as_is() {
        // Produce a real processed page (plain JPEG, no EXIF) and export it.
        let src = temp_file("easyscan_pass_src.jpg", &{
            let img = RgbImage::from_fn(60, 40, |x, y| Rgb([(x * 5 + y) as u8, (y * 7) as u8, 90]));
            let mut jpeg = Vec::new();
            JpegEncoder::new_with_quality(&mut jpeg, 90)
                .encode_image(&DynamicImage::ImageRgb8(img))
                .unwrap();
            jpeg
        });
        let bytes = std::fs::read(&src).unwrap();
        let out = std::env::temp_dir().join("easyscan_pass.pdf");
        export_pdf_file(
            vec![src.display().to_string()],
            out.display().to_string(),
            PdfPageSize::A4,
            80,
        )
        .unwrap();
        let pdf = std::fs::read(&out).unwrap();
        // A landscape page on A4 rotates the page box...
        let s = String::from_utf8_lossy(&pdf);
        assert!(s.contains("/MediaBox [0 0 841.89 595.28]"), "{s}");
        // ...and the JPEG must be embedded untouched.
        assert!(
            pdf.windows(bytes.len()).any(|w| w == bytes.as_slice()),
            "process_page_file output should pass through without a re-encode"
        );
        std::fs::remove_file(&src).unwrap();
        std::fs::remove_file(&out).unwrap();
    }

    /// Memory/time for a 20-page export. Hand-run for the report:
    /// `cargo test -p easyscan_bridge --release export_pdf_timing -- --ignored --nocapture`
    #[test]
    #[ignore = "hand-run release bench for size/time numbers"]
    fn export_pdf_timing_20_pages() {
        let img = RgbImage::from_fn(4000, 3000, |x, y| {
            let page = (600..3400).contains(&x) && (300..2700).contains(&y);
            if page {
                Rgb([248, 248, 246])
            } else {
                Rgb([70, 70, 70])
            }
        });
        let src = temp_file("easyscan_20p_src.jpg", &{
            let mut jpeg = Vec::new();
            JpegEncoder::new_with_quality(&mut jpeg, 85)
                .encode_image(&DynamicImage::ImageRgb8(img))
                .unwrap();
            jpeg
        });
        let out = std::env::temp_dir().join("easyscan_20p.pdf");
        let start = Instant::now();
        let paths: Vec<String> = (0..20).map(|_| src.display().to_string()).collect();
        export_pdf_file(
            paths,
            out.display().to_string(),
            PdfPageSize::FitToImage,
            80,
        )
        .unwrap();
        let secs = start.elapsed();
        let pdf_len = std::fs::metadata(&out).unwrap().len();
        eprintln!(
            "20 × 12MP JPEG export (pass-through): {pdf_len} bytes in {secs:?} — no page was decoded"
        );
        std::fs::remove_file(&src).unwrap();
        std::fs::remove_file(&out).unwrap();
    }

    /// Desktop release numbers for the 12MP path; hand-run:
    /// `cargo test -p easyscan_bridge --release file_api_timing -- --ignored --nocapture`
    #[test]
    #[ignore = "hand-run release bench for size/speed numbers"]
    fn file_api_timing_12mp() {
        // 4000×3000 with a page-shaped bright region on a noisy desk.
        let img = image::RgbImage::from_fn(4000, 3000, |x, y| {
            let page = (600..3400).contains(&x) && (300..2700).contains(&y);
            let noise = ((x / 13 % 4 == 0) as u8) * 10;
            let dark = 60 + noise;
            if page {
                image::Rgb([248, 248, 246])
            } else {
                image::Rgb([dark, dark, dark])
            }
        });
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 85)
            .encode_image(&DynamicImage::ImageRgb8(img))
            .unwrap();
        let src = temp_file("easyscan_bench12mp.jpg", &jpeg);
        {
            let decoder = ImageReader::open(&src).unwrap().into_decoder().unwrap();
            let dims = decoder.dimensions();
            assert_eq!((4000, 3000), dims);
        }
        let out = std::env::temp_dir().join("easyscan_bench12mp_out.jpg");

        let decode_start = Instant::now();
        let detection = detect_page_file(src.display().to_string(), 0).unwrap();
        let detect_s = decode_start.elapsed().as_secs_f32();
        eprintln!(
            "12MP detect+decode:      {detect_s:.3} s (found {} conf {:.2})",
            detection.found, detection.confidence
        );

        let proc_start = Instant::now();
        let size = process_page_file(
            src.display().to_string(),
            detection.corners.clone(),
            ScanFilter::Grayscale,
            0,
            None,
            out.display().to_string(),
            None,
            80,
        )
        .unwrap();
        let process_s = proc_start.elapsed().as_secs_f32();
        eprintln!(
            "12MP process full-res:   {process_s:.3} s -> {}×{} JPEG at {:?}",
            size.width,
            size.height,
            out.display()
        );
        eprintln!(
            "12MP detect+process:     {:.3} s (target < 1.5 s)",
            detect_s + process_s
        );

        let pdf_start = Instant::now();
        export_pdf_file(
            vec![src.display().to_string()],
            std::env::temp_dir()
                .join("easyscan_bench.pdf")
                .display()
                .to_string(),
            PdfPageSize::FitToImage,
            80,
        )
        .unwrap();
        let pdf_s = pdf_start.elapsed().as_secs_f32();
        eprintln!("12MP export pdf only:    {pdf_s:.3} s");
        eprintln!(
            "12MP detect+process+pdf: {:.3} s",
            detect_s + process_s + pdf_s
        );
        std::fs::remove_file(&src).unwrap();
        std::fs::remove_file(&out).unwrap();
        std::fs::remove_file(std::env::temp_dir().join("easyscan_bench.pdf")).unwrap();
    }
}
