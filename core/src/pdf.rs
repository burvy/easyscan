//! Multi-page PDF export: pages (images) in, one PDF out.
//!
//! Each page is embedded as a JPEG (`DCTDecode`) image using the smallest
//! colorspace that preserves it: grayscale pages become `DeviceGray` JPEGs,
//! colour pages `DeviceRGB`. The PDF objects are written by hand — a scanner
//! PDF needs roughly ten lines of PDF per page and no fonts, so a dependency
//! (e.g. `printpdf`) would cost bundle size for nothing.
//!
//! No text layer, encryption, or non-JPEG compression (non-goals in DESIGN.md).

use image::codecs::jpeg::JpegEncoder;
use image::DynamicImage;

/// Page geometry for exported pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageSize {
    /// Page = image size at the export DPI. Round trip: an image printed at
    /// [`PdfOptions::dpi`] comes out exactly as scanned, no padding.
    FitToImage,
    /// A4 (portrait by default; landscape when the image is wider than it
    /// is tall). Image scaled to fit, aspect kept, centred.
    A4,
    /// US Letter (portrait by default; landscape when the image is wider
    /// than it is tall). Image scaled to fit, aspect kept, centred.
    Letter,
}

/// Options for [`export_pdf`].
#[derive(Clone, Copy, Debug)]
pub struct PdfOptions {
    /// JPEG quality, 1–100. Lower means smaller files.
    pub jpeg_quality: u8,
    /// Scan resolution used to turn pixels into points for
    /// [`PageSize::FitToImage`] (72 pt per inch).
    pub dpi: u32,
    /// Target page size.
    pub page_size: PageSize,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            jpeg_quality: 75,
            dpi: 150,
            page_size: PageSize::FitToImage,
        }
    }
}

/// Stateless: ordered pages in, one PDF out as bytes.
///
/// Precondition: `pages` is non-empty — the app never exports zero pages,
/// and some viewers reject a 0-page PDF.
///
/// Builds a minimal PDF: catalog, page tree, and per page a JPEG image
/// XObject plus a content stream that draws it. Objects get strictly
/// increasing numbers, and their byte offsets are recorded as they are
/// written so the xref table at the end is exact.
pub fn export_pdf(pages: &[DynamicImage], options: &PdfOptions) -> Vec<u8> {
    debug_assert!(!pages.is_empty(), "export of an empty scan produces a PDF most viewers reject");
    let mut buf = Vec::new();

    // Binary marker so viewers treat the rest as bytes, not latin-1 text.
    buf.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");

    let mut offsets: Vec<usize> = Vec::with_capacity(2 + 3 * pages.len() + 1);

    // Object 1: catalog.
    start_object(&mut buf, &mut offsets, 1);
    buf.extend_from_slice(b"<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    // Object 2: page tree. Pages take object numbers 3, 6, 9, ... (page,
    // content stream, image XObject each get three consecutive numbers), so
    // the kids list can be built before any page is written.
    let kids: String = pages
        .iter()
        .enumerate()
        .map(|(i, _)| format!("{} 0 R ", 3 + 3 * i))
        .collect();
    let tree = format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>\nendobj\n",
        kids.trim_end(),
        pages.len()
    );
    start_object(&mut buf, &mut offsets, 2);
    buf.extend_from_slice(tree.as_bytes());

    for (i, page) in pages.iter().enumerate() {
        let page_num = 3 + 3 * i;
        let content_num = page_num + 1;
        let image_num = page_num + 2;

        let (jpeg, colorspace) = encode_jpeg(page, options.jpeg_quality);
        let (media, (x, y, w, h)) = layout(options, page.width(), page.height());

        start_object(&mut buf, &mut offsets, page_num);
        let page_obj = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.2} {:.2}] \
             /Resources << /ProcSet [/PDF /ImageB /ImageC] \
             /XObject << /Im{} {} 0 R >> >> /Contents {} 0 R >>\nendobj\n",
            media.0, media.1, i + 1, image_num, content_num
        );
        buf.extend_from_slice(page_obj.as_bytes());

        // The image maps to the unit square, so this cm is the entire
        // scaling/aspect handling: place (x, y), scale to (w, h). The q/Q
        // pair keeps the stream balanced for strict viewers and preflight.
        start_object(&mut buf, &mut offsets, content_num);
        let cmd = format!("q {:.2} 0 0 {:.2} {:.2} {:.2} cm /Im{} Do Q\n", w, h, x, y, i + 1);
        let content = format!("<< /Length {} >>\nstream\n{}\nendstream\nendobj\n", cmd.len(), cmd);
        buf.extend_from_slice(content.as_bytes());

        start_object(&mut buf, &mut offsets, image_num);
        let image_obj = format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /{cs} \
             /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            page.width(),
            page.height(),
            jpeg.len(),
            cs = colorspace
        );
        buf.extend_from_slice(image_obj.as_bytes());
        buf.extend_from_slice(&jpeg);
        buf.extend_from_slice(b"\nendstream\nendobj\n");
    }

    // Title/Producer — the only metadata this export writes.
    let info_num = 3 + 3 * pages.len();
    start_object(&mut buf, &mut offsets, info_num);
    buf.extend_from_slice(b"<< /Title (EasyScan scan) /Producer (EasyScan Core 0.1.0) >>\nendobj\n");

    // xref: entry i is object i; object 0 is the free-list head.
    let xref_start = buf.len();
    buf.extend_from_slice(format!("xref\n0 {}\n", offsets.len() + 1).as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for off in &offsets {
        buf.extend_from_slice(format!("{:010} 00000 n \n", off).as_bytes());
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info {} 0 R >>\nstartxref\n{}\n%%EOF\n",
            offsets.len() + 1,
            info_num,
            xref_start
        )
        .as_bytes(),
    );
    buf
}

fn start_object(buf: &mut Vec<u8>, offsets: &mut Vec<usize>, num: usize) {
    offsets.push(buf.len());
    buf.extend_from_slice(format!("{} 0 obj\n", num).as_bytes());
}

/// Encode one page as a JPEG, choosing DeviceGray for grayscale pages to
/// shrink the file. JPEG encoding has no failure mode for our inputs.
fn encode_jpeg(page: &DynamicImage, quality: u8) -> (Vec<u8>, &'static str) {
    let quality = quality.clamp(1, 100);
    let (source, colorspace) = match page {
        DynamicImage::ImageLuma8(_) | DynamicImage::ImageLumaA8(_) | DynamicImage::ImageLuma16(_) => {
            (DynamicImage::ImageLuma8(page.to_luma8()), "DeviceGray")
        }
        _ => (DynamicImage::ImageRgb8(page.to_rgb8()), "DeviceRGB"),
    };
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, quality)
        .encode_image(&source)
        .expect("JPEG encoding cannot fail");
    (jpeg, colorspace)
}

/// A4 portrait size in points; see [`orient`] for landscape.
const A4: (f64, f64) = (595.28, 841.89);
/// US Letter portrait size in points; see [`orient`] for landscape.
const LETTER: (f64, f64) = (612.0, 792.0);

/// Page media box in points, then the rectangle (x, y, w, h) where the page
/// image is drawn — scaled to fit, aspect kept, centred.
fn layout(options: &PdfOptions, w_px: u32, h_px: u32) -> ((f64, f64), (f64, f64, f64, f64)) {
    // dpi 0 would make infinite/NaN points; clamp instead of failing an export.
    let dpi = options.dpi.max(1) as f64;
    let image = (w_px as f64 * 72.0 / dpi, h_px as f64 * 72.0 / dpi);
    match options.page_size {
        PageSize::FitToImage => (image, (0.0, 0.0, image.0, image.1)),
        PageSize::A4 => fit_in(orient(A4, image), image),
        PageSize::Letter => fit_in(orient(LETTER, image), image),
    }
}

/// Landscape page box when the page image is wider than it is tall, so a
/// landscape worksheet fills the page (the page itself stays unrotated —
/// no /Rotate).
fn orient(page: (f64, f64), image: (f64, f64)) -> (f64, f64) {
    if image.0 > image.1 {
        (page.1, page.0)
    } else {
        page
    }
}

fn fit_in(page: (f64, f64), image: (f64, f64)) -> ((f64, f64), (f64, f64, f64, f64)) {
    let scale = (page.0 / image.0).min(page.1 / image.1);
    let (w, h) = (image.0 * scale, image.1 * scale);
    (page, ((page.0 - w) / 2.0, (page.1 - h) / 2.0, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, File};
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::Instant;

    use image::{Luma, Rgb, RgbImage};

    fn gray_page(n: u32) -> DynamicImage {
        // Non-trivial content, so the JPEG carries real data and a real size.
        let size = 400 + n * 10;
        let mut img = vec![0u8; (size * size) as usize];
        for (i, px) in img.iter_mut().enumerate() {
            let (x, y) = (i % size as usize, i / size as usize);
            *px = ((x * 251 + y * 199) % 256) as u8;
        }
        DynamicImage::ImageLuma8(image::GrayImage::from_raw(size, size, img).unwrap())
    }

    fn color_page(n: u32) -> DynamicImage {
        let mut img = RgbImage::new(400 + n * 10, 400);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = Rgb([x as u8, y as u8, (x + y) as u8]);
        }
        DynamicImage::ImageRgb8(img)
    }

    #[test]
    fn starts_and_ends_with_markers() {
        for pages in [vec![gray_page(1)], vec![gray_page(1), gray_page(2)]] {
            let pdf = export_pdf(&pages, &PdfOptions::default());
            assert!(pdf.starts_with(b"%PDF"));
            assert!(pdf.ends_with(b"%%EOF\n"));
        }
    }

    #[test]
    fn page_count_matches_input() {
        for pages in [vec![gray_page(1)], vec![gray_page(1), gray_page(2)]] {
            let pdf = export_pdf(&pages, &PdfOptions::default());
            let s = String::from_utf8_lossy(&pdf);
            // "/Type /Page" with a trailing space, so "/Type /Pages" can't match.
            assert_eq!(s.match_indices("/Type /Page ").count(), pages.len());
            let start = s.find("/Kids [").expect("kids") + "/Kids [".len();
            let kids = &s[start..s[start..].find(']').unwrap() + start];
            assert_eq!(kids.matches(" 0 R").count(), pages.len());
        }
    }

    #[test]
    fn xref_offsets_are_correct() {
        let pages = vec![gray_page(1), color_page(1)];
        let pdf = export_pdf(&pages, &PdfOptions::default());

        let start = pdf
            .windows(b"startxref\n".len())
            .rposition(|w| w == *b"startxref\n")
            .expect("startxref")
            + "startxref\n".len();
        let eol = pdf[start..].iter().position(|&b| b == b'\n').unwrap();
        let xref_off: usize =
            std::str::from_utf8(&pdf[start..start + eol]).unwrap().parse().expect("numeric startxref");
        assert_eq!(&pdf[xref_off..xref_off + 4], b"xref", "startxref must point at the table");

        let mut line = xref_off + "xref\n".len();
        let header_end = line + pdf[line..].iter().position(|&b| b == b'\n').unwrap();
        let line_str = std::str::from_utf8(&pdf[line..header_end]).unwrap();
        let total: usize = line_str.split(' ').nth(1).unwrap().parse().unwrap();
        line = header_end + 1;

        let mut objects = 0;
        for object in 0..total {
            let entry = &pdf[line..line + 20];
            if object == 0 {
                assert_eq!(&entry[11..16], b"65535", "entry 0 is the free-list head");
                assert_eq!(&entry[17..18], b"f");
            } else {
                assert_eq!(&entry[17..18], b"n");
                let off: usize = std::str::from_utf8(&entry[..10]).unwrap().trim().parse().unwrap();
                let expect = format!("{} 0 obj", object);
                assert_eq!(
                    &pdf[off..off + expect.len()],
                    expect.as_bytes(),
                    "xref entry {} must point at its object",
                    object
                );
                objects += 1;
            }
            line += 20;
        }
        // Catalog, page tree, 2 pages × 3 objects, info.
        assert_eq!(objects, 2 + 2 * 3 + 1);
    }

    #[test]
    fn grayscale_pages_use_device_gray() {
        let pdf = export_pdf(&[gray_page(1)], &PdfOptions::default());
        let s = String::from_utf8_lossy(&pdf);
        assert!(s.contains("/ColorSpace /DeviceGray"));
        assert!(!s.contains("/ColorSpace /DeviceRGB"), "gray page must not say DeviceRGB");
    }

    #[test]
    fn color_pages_use_device_rgb() {
        let pdf = export_pdf(&[color_page(1)], &PdfOptions::default());
        let s = String::from_utf8_lossy(&pdf);
        assert!(s.contains("/ColorSpace /DeviceRGB"));
    }

    #[test]
    fn fixed_page_sizes_keep_aspect() {
        // A wide 2:1 image on Letter: the page turns landscape (792 × 612)
        // so the worksheet fills it; width binds at scale 792/400 = 1.98.
        let wide = DynamicImage::ImageRgb8(RgbImage::new(400, 200));
        let options = PdfOptions { page_size: PageSize::Letter, ..Default::default() };
        let pdf = export_pdf(&[wide], &options);
        let s = String::from_utf8_lossy(&pdf);
        assert!(s.contains("/MediaBox [0 0 792.00 612.00]"), "{s}");
        assert!(s.contains("q 792.00 0 0 396.00 0.00 108.00 cm"), "{s}");

        // And a tall 1:2 image stays on portrait Letter (612 × 792).
        let tall = DynamicImage::ImageRgb8(RgbImage::new(200, 400));
        let pdf = export_pdf(&[tall], &options);
        let s = String::from_utf8_lossy(&pdf);
        assert!(s.contains("/MediaBox [0 0 612.00 792.00]"), "{s}");
        assert!(s.contains("q 396.00 0 0 792.00 108.00 0.00 cm"), "{s}");
    }

    /// Every content stream must be balanced: it starts with `q`, and the
    /// only other streams are JPEGs (SOI marker).
    #[test]
    fn content_streams_start_with_q() {
        let pdf = export_pdf(
            &[gray_page(1), color_page(1), gray_page(2)],
            &PdfOptions::default(),
        );
        let mut content_streams = 0;
        let mut at = 0;
        // Streams open on their own line: "\nstream\n". That never matches
        // inside "endstream\n" (preceded by the 'd').
        while let Some(found) = pdf[at..]
            .windows(b"\nstream\n".len())
            .position(|w| w == *b"\nstream\n")
            .map(|p| p + at)
        {
            at = found + "\nstream\n".len();
            let body = &pdf[at..];
            assert!(
                body.starts_with(b"q ") || body.starts_with(&[0xFF, 0xD8]),
                "stream must be a q-paired content stream or a JPEG: {:?}",
                &body[..20.min(body.len())]
            );
            content_streams += usize::from(body.starts_with(b"q "));
        }
        assert_eq!(content_streams, 3);
    }

    /// dpi 0 would put inf/NaN in the MediaBox; it must never leak through.
    #[test]
    fn zero_dpi_is_clamped_not_nan() {
        let options = PdfOptions { dpi: 0, ..Default::default() };
        let pdf = export_pdf(&[gray_page(1)], &options);
        let s = String::from_utf8_lossy(&pdf);
        assert!(!s.contains("NaN") && !s.contains("inf"), "{s} contains a non-finite number");
        // dpi clamps to 1: a 410 px page → 410 × 72 = 29520 pt.
        assert!(s.contains("/MediaBox [0 0 29520.00 29520.00]"), "unexpected mediabox, dpi not clamped");
    }

    /// Writes target/sample.pdf (workspace target dir) from 3 generated
    /// images for eyeballing in Chrome/Acrobat/Android viewers.
    #[test]
    fn three_page_sample_for_a_browser() {
        let pages = vec![gray_page(1), gray_page(3), gray_page(7)];
        let pdf = export_pdf(&pages, &PdfOptions::default());

        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("target");
        create_dir_all(&dir).unwrap();
        let path = dir.join("sample.pdf");
        File::create(&path).unwrap().write_all(&pdf).unwrap();

        eprintln!("sample.pdf written to {:?} — {} bytes", path.canonicalize().unwrap(), pdf.len());
        assert!(pdf.len() > 1000, "3 pages of noise should make a real PDF");
    }

    /// Phone-size rough numbers. Hand-run for size/speed data:
    /// `cargo test --full_res_bench_numbers -- --ignored --nocapture`.
    #[test]
    #[ignore = "slow full-res bench; run by hand for size/speed numbers"]
    fn full_res_bench_numbers() {
        let pages: Vec<DynamicImage> = (0..3)
            .map(|i| {
                let mut img = image::GrayImage::new(4000, 3000);
                for y in 0..3000 {
                    for x in 0..4000 {
                        img.put_pixel(x, y, Luma([(((x / 4 + y / 4 + i * 17) % 256) as u8)]));
                    }
                }
                DynamicImage::ImageLuma8(img)
            })
            .collect();
        let start = Instant::now();
        let pdf = export_pdf(&pages, &PdfOptions::default());
        eprintln!(
            "3 × (4000×3000) gray pages at q75: {} bytes total in {:?}",
            pdf.len(),
            start.elapsed()
        );
        assert!(pdf.starts_with(b"%PDF"));
    }
}
