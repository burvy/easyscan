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

/// One page for export.
///
/// A [`PageInput::Jpeg`] is embedded byte-for-byte — no decode, no re-encode.
/// A [`PageInput::Image`] is JPEG-encoded as today. The bridge picks the
/// first when the bytes are an embeddable baseline JPEG, the second
/// otherwise.
#[derive(Debug)]
pub enum PageInput {
    /// Validated at construction: a `Jpeg::new` caller knows the bytes are
    /// a baseline JPEG with no sideways EXIF, so embedding is safe.
    Jpeg(Jpeg),
    /// Encoded (and colorspace-minimized) at export time.
    Image(DynamicImage),
}

/// An already-encoded JPEG to embed as-is. Facts (size, component count,
/// EXIF orientation) are parsed once at construction, and the writer
/// trusts them.
#[derive(Debug)]
pub struct Jpeg {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    grayscale: bool,
}

impl Jpeg {
    /// Wraps JPEG bytes for embedding. Returns Err (with a readable
    /// reason) when they are not an embeddable baseline JPEG and the caller
    /// should decode/re-encode instead: not JPEG, progressive/lossless/
    /// arithmetic-coded or CMYK, or EXIF orientation other than 1 —
    /// rotation must be baked into the pixels before this point.
    pub fn new(bytes: Vec<u8>) -> Result<Jpeg, String> {
        match scan_jpeg(&bytes) {
            None => Err("not a baseline JPEG (wrong magic, no SOF0/SOF1 marker, progressive, or truncated)".into()),
            Some(info) if info.components == 1 => {
                check_exif(&bytes)?;
                Ok(Jpeg { grayscale: true, width: info.width, height: info.height, bytes })
            }
            Some(info) if info.components == 3 => {
                check_exif(&bytes)?;
                Ok(Jpeg { grayscale: false, width: info.width, height: info.height, bytes })
            }
            Some(info) => Err(format!(
                "{}-component JPEG (CMYK or unusable for our purposes); decode and re-encode instead",
                info.components
            )),
        }
    }
}

/// Rejects inputs whose EXIF orientation would misrender if embedded as-is.
fn check_exif(bytes: &[u8]) -> Result<(), String> {
    let orientation = exif_orientation(bytes);
    if orientation == 0 || orientation == 1 {
        return Ok(());
    }
    if !(1..=8).contains(&orientation) {
        Err(format!("unusual EXIF orientation {orientation}"))
    } else {
        Err(format!(
            "EXIF orientation {orientation} (sideways photo); bake it into the pixels first"
        ))
    }
}

struct Scan {
    width: u32,
    height: u32,
    components: u16,
}

/// SOF0/SOF1 facts, or None when the JPEG can't be embedded as-is.
///
/// Convention below: `m` is the index of the marker *byte* (the byte after
/// the 0xFF), and a segment's 2-byte length counts its own two bytes, so
/// the next 0xFF sits at `m + seg_len + 1`.
fn scan_jpeg(jpeg: &[u8]) -> Option<Scan> {
    if jpeg.len() < 4 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        return None;
    }
    let mut i = 2; // bytes consumed: SOI
    while i < jpeg.len() {
        if jpeg[i] != 0xFF {
            return None;
        }
        let mut m = i + 1;
        while m < jpeg.len() && jpeg[m] == 0xFF {
            m += 1; // FF fill bytes before the marker byte
        }
        let &marker = jpeg.get(m)?;
        match marker {
            // TEM and standalone markers carry no length.
            0x01 | 0xD0..=0xD9 => {
                i = m + 2;
                continue;
            }
            0x00 => return None, // byte stuffing — entropy data, no header left
            0xDA => return None, // entropy-coded scan start before any SOF
            _ => {}
        }
        let seg_len = jpeg
            .get(m + 1..m + 3)
            .map(|b| u16::from_be_bytes([b[0], b[1]]) as usize)
            .filter(|l| m + 1 + l <= jpeg.len())?;
        if marker == 0xC0 || marker == 0xC1 {
            // SOF: precision(m+3) height(2) width(2) components(1)…
            let info = jpeg.get(m + 2..m + 9)?;
            return Some(Scan {
                width: u16::from_be_bytes([info[4], info[5]]) as u32,
                height: u16::from_be_bytes([info[2], info[3]]) as u32,
                components: info[6] as u16,
            });
        }
        i = m + 1 + seg_len;
    }
    None
}

/// EXIF Orientation value in the first APP1 EXIF segment (0 = none found).
fn exif_orientation(jpeg: &[u8]) -> u16 {
    let mut i = 2;
    while i < jpeg.len() {
        if jpeg[i] != 0xFF {
            return 0;
        }
        let mut m = i + 1;
        while m < jpeg.len() && jpeg[m] == 0xFF {
            m += 1;
        }
        let Some(&marker) = jpeg.get(m) else { return 0 };
        match marker {
            0x01 | 0xD0..=0xD9 => {
                i = m + 2;
                continue;
            }
            0x00 | 0xDA => return 0, // header region over
            _ => {}
        }
        let Some(seg_len) = jpeg
            .get(m + 1..m + 3)
            .map(|b| u16::from_be_bytes([b[0], b[1]]) as usize)
            .filter(|l| m + 1 + l <= jpeg.len())
        else {
            return 0;
        };
        if marker == 0xE1 {
            let o = jpeg.get(m + 3..m + 1 + seg_len).and_then(parse_exif_orientation);
            if let Some(o) = o {
                return o;
            }
        }
        i = m + 1 + seg_len;
    }
    0
}

/// Orientation tag inside "Exif\0\0" + TIFF header + IFD0 entries.
fn parse_exif_orientation(data: &[u8]) -> Option<u16> {
    if data.len() < 6 + 8 || &data[0..6] != b"Exif\0\0" {
        return None;
    }
    let t = &data[6..];
    let little = t.len() >= 2 && t[0..2] == *b"II";
    let u10 = |off: usize| if little { u16::from_le_bytes([t[off], t[off + 1]]) } else { u16::from_be_bytes([t[off], t[off + 1]]) };
    let u30 = |off: usize| if little {
        u32::from_le_bytes([t[off], t[off + 1], t[off + 2], t[off + 3]])
    } else {
        u32::from_be_bytes([t[off], t[off + 1], t[off + 2], t[off + 3]])
    };
    let ifd = u30(4) as usize;
    if ifd + 2 > t.len() {
        return None;
    }
    let entry_count = u10(ifd) as usize;
    for e in 0..entry_count {
        let entry = ifd + 2 + 12 * e;
        if entry + 12 > t.len() {
            return None;
        }
        if u10(entry) == 0x0112 {
            // SHORT value (count 1) sits inline in the value field.
            return Some(u10(entry + 8));
        }
    }
    None
}

struct PageImage {
    jpeg: Vec<u8>,
    colorspace: &'static str,
    width_px: u32,
    height_px: u32,
}

/// Turn one [`PageInput`] into the JPEG + facts to embed. Runs on one page
/// at a time inside the writer, so peak memory holds a single decoded page.
fn realize(page: PageInput, quality: u8) -> Result<PageImage, String> {
    match page {
        PageInput::Image(img) => {
            let (jpeg, colorspace) = encode_jpeg(&img, quality);
            Ok(PageImage {
                jpeg,
                colorspace,
                width_px: img.width(),
                height_px: img.height(),
            })
        }
        PageInput::Jpeg(j) => Ok(PageImage {
            colorspace: if j.grayscale { "DeviceGray" } else { "DeviceRGB" },
            jpeg: j.bytes,
            width_px: j.width,
            height_px: j.height,
        }),
    }
}

/// The one-shot variant of [`export_pdf_pages`]: ordered pages in, one
/// PDF out as bytes.
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
    let mut out = Vec::new();
    let inputs = pages.iter().map(|p| PageInput::Image(p.clone()));
    export_pdf_pages(inputs, options, &mut out)
        .expect("in-memory images cannot fail to encode; see encode_jpeg");
    out
}

/// One page at a time, into `out` — the peak-memory export path used for
/// real scans.
///
/// `pages.len()` decides /Count and the Kids list before any page is
/// written, so the iterator's len must be truthful. Every page is pulled
/// and written before the next one is pulled; only one page's decode lives
/// at a time.
pub fn export_pdf_pages<W: std::io::Write>(
    pages: impl ExactSizeIterator<Item = PageInput>,
    options: &PdfOptions,
    out: &mut W,
) -> Result<(), String> {
    let count = pages.len();
    if count == 0 {
        return Err("no pages".into());
    }
    let mut d = Doc::with_capacity(count);

    // Binary marker so viewers treat the rest as bytes, not latin-1 text.
    d.buf.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");

    // Object 1: catalog.
    d.start_object(1);
    d.buf.extend_from_slice(b"<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    // Object 2: page tree. Pages take object numbers 3, 6, 9, ... (page,
    // content stream, image XObject each get three consecutive numbers), so
    // the kids list can be built before any page is written.
    let kids: String = (0..count)
        .map(|i| format!("{} 0 R ", 3 + 3 * i))
        .collect();
    let tree = format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>\nendobj\n",
        kids.trim_end(),
        count
    );
    d.start_object(2);
    d.buf.extend_from_slice(tree.as_bytes());

    for (i, page) in pages.enumerate() {
        write_page(&mut d, i, realize(page, options.jpeg_quality)?, options);
    }

    // Title/Producer — the only metadata this export writes.
    let info_num = 3 + 3 * count;
    d.start_object(info_num);
    d.buf.extend_from_slice(b"<< /Title (EasyScan scan) /Producer (EasyScan Core 0.1.0) >>\nendobj\n");
    d.finish_trailer(info_num);

    out.write_all(&d.buf).map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())?;
    Ok(())
}

struct Doc {
    buf: Vec<u8>,
    offsets: Vec<usize>,
}

impl Doc {
    fn with_capacity(count: usize) -> Doc {
        Doc {
            buf: Vec::with_capacity(4096 + count * 340 * 1024),
            offsets: Vec::with_capacity(2 + 3 * count + 1),
        }
    }

    fn start_object(&mut self, num: usize) {
        self.offsets.push(self.buf.len());
        self.buf.extend_from_slice(format!("{} 0 obj\n", num).as_bytes());
    }

    fn finish_trailer(&mut self, info_num: usize) {
        // xref: entry i is object i; object 0 is the free-list head.
        let xref_start = self.buf.len();
        self.buf.extend_from_slice(format!("xref\n0 {}\n", self.offsets.len() + 1).as_bytes());
        self.buf.extend_from_slice(b"0000000000 65535 f \n");
        let offsets = self.offsets.clone();
        for off in offsets {
            self.buf.extend_from_slice(format!("{:010} 00000 n \n", off).as_bytes());
        }
        self.buf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R /Info {} 0 R >>\nstartxref\n{}\n%%EOF\n",
                self.offsets.len() + 1,
                info_num,
                xref_start
            )
            .as_bytes(),
        );
    }
}

fn write_page(d: &mut Doc, index: usize, image: PageImage, options: &PdfOptions) {
    let page_num = 3 + 3 * index;
    let content_num = page_num + 1;
    let image_num = page_num + 2;

    let (media, (x, y, w, h)) = layout(options, image.width_px, image.height_px);

    d.start_object(page_num);
    let page_obj = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.2} {:.2}] \
         /Resources << /ProcSet [/PDF /ImageB /ImageC] \
         /XObject << /Im{} {} 0 R >> >> /Contents {} 0 R >>\nendobj\n",
        media.0, media.1, index + 1, image_num, content_num
    );
    d.buf.extend_from_slice(page_obj.as_bytes());

    // The image maps to the unit square, so this cm is the entire
    // scaling/aspect handling: place (x, y), scale to (w, h). The q/Q
    // pair keeps the stream balanced for strict viewers and preflight.
    d.start_object(content_num);
    let cmd = format!("q {:.2} 0 0 {:.2} {:.2} {:.2} cm /Im{} Do Q\n", w, h, x, y, index + 1);
    let content = format!("<< /Length {} >>\nstream\n{}\nendstream\nendobj\n", cmd.len(), cmd);
    d.buf.extend_from_slice(content.as_bytes());

    d.start_object(image_num);
    let image_obj = format!(
        "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /{cs} \
         /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
        image.width_px,
        image.height_px,
        image.jpeg.len(),
        cs = image.colorspace
    );
    d.buf.extend_from_slice(image_obj.as_bytes());
    d.buf.extend_from_slice(&image.jpeg);
    d.buf.extend_from_slice(b"\nendstream\nendobj\n");
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
        // Catalog, page tree, 2 pages × 3 objects, info.
        assert_eq!(2 + 2 * 3 + 1, xref_in_use_objects(&pdf));
    }

    /// Parses the xref table and verifies every in-use entry points at
    /// `"<n> 0 obj"` in `pdf`. Returns the number of in-use objects.
    fn xref_in_use_objects(pdf: &[u8]) -> usize {
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
        objects
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

    // ===== 012: pass-through JPEG export =====

    fn encode_jpeg_bytes(img: &DynamicImage, quality: u8) -> Vec<u8> {
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality)
            .encode_image(img)
            .unwrap();
        jpeg
    }

    fn color_jpeg_bytes() -> Vec<u8> {
        let img = RgbImage::from_fn(40, 30, |x, y| Rgb([(x * 6) as u8, (y * 8) as u8, 100]));
        encode_jpeg_bytes(&DynamicImage::ImageRgb8(img), 85)
    }

    /// A real 1-component grayscale baseline JPEG (40×30, quality 85,
    /// Pillow-generated). The image crate's Luma8 JPEGs are 3-component
    /// YCbCr, so this constant is the only way to exercise the DeviceGray
    /// pass-through honestly.
    fn gray_jpeg_bytes() -> Vec<u8> {
        const GRAY_JPEG_HEX: &str = "\
ffd8ffe000104a46494600010100000100010000ffdb0043000503040404030504040405050506070c08070707070f0b0b090c110f12\
12110f111113161c1713141a1511111821181a1d1d1f1f1f13172224221e241c1e1f1effc0000b08001e002801011100ffc4001f0000\
010501010101010100000000000000000102030405060708090a0bffc400b5100002010303020403050504040000017d010203000411\
05122131410613516107227114328191a1082342b1c11552d1f02433627282090a161718191a25262728292a3435363738393a434445\
464748494a535455565758595a636465666768696a737475767778797a838485868788898a92939495969798999aa2a3a4a5a6a7a8a9\
aab2b3b4b5b6b7b8b9bac2c3c4c5c6c7c8c9cad2d3d4d5d6d7d8d9dae1e2e3e4e5e6e7e8e9eaf1f2f3f4f5f6f7f8f9faffda00080101\
00003f00f01f0df86feefeeff4af4cf0df86feefeeff004af4df0df86feefeeff4af4df0df86feefeeff004ae67e35fc69f0dfc2af3b\
c3fa7db7f6cf8b9adbcc8ad5706ded19b1b0dcb0208ca92e1172c4019281d5ebc97c37e1bfbbfbbfd2bd37c37e1bfbbfbbfd2bd1b49d1\
ad6c2c65bfbf9a1b5b4b689a69e799c2471228cb3b31e15400492780057cf3f1aff006929afbcef0cfc2697cbb09adbcbb9d79a1922b8\
2cd8245b06da63c2e54bb2eecb128176abb786786fc37f77f77fa57d99e1bf0dfddfddfe95d078b3c43e11f86de1d7d6bc57a9436aa22\
67b7b40ca6e6f0aed05218c905db2e80f65dc0b155c91f1ff00c5df8b7e2ef8b37cb6e566d07c38b12a2e8d6f74cf1ca72ac5e76017ce\
6dea0ae54040a3033b99b33c37e1bfbbfbbfd2bd37c37e1bfbbfbbfd2bda3e3d7c51d3be0df876c8a6932ea3ad6af14e34b5200b6478b\
cb0cd31dc1b68f3410aa096da46533b87c65adeb1e22f1ef88bfe120f176b12eaba8b449089645545445e8888802a2f24e14004b313c9\
24f5be1bd0a1f97eed7a6f86f4287e5fbb5e9be1bd0a1f97eed7ffd9";
        (0..GRAY_JPEG_HEX.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&GRAY_JPEG_HEX[i..i + 2], 16).expect("valid hex"))
            .collect()
    }

    /// Injects an APP1 EXIF segment (little-endian TIFF, one Orientation
    /// short) into a JPEG, like a camera would write it.
    fn with_exif_orientation(jpeg: &[u8], orientation: u16) -> Vec<u8> {
        assert!(jpeg.starts_with(&[0xFF, 0xD8]));
        let mut tiff: Vec<u8> = b"II\x2A\x00\x08\x00\x00\x00".to_vec(); // LE, IFD0 at 8
        tiff.extend_from_slice(&1u16.to_le_bytes()); // one entry
        tiff.extend_from_slice(&[0x12, 0x01]); // tag 0x0112 (Orientation)
        tiff.extend_from_slice(&3u16.to_le_bytes()); // type SHORT
        tiff.extend_from_slice(&1u32.to_le_bytes()); // count
        tiff.extend_from_slice(&orientation.to_le_bytes()); // inline value
        tiff.extend_from_slice(&[0, 0]);
        tiff.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&tiff);
        let mut out = vec![0xFF, 0xD8];
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&((app1.len() as u16 + 2).to_be_bytes()));
        out.extend_from_slice(&app1);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    /// A fake JPEG header with one SOF-marker segment of `components`
    /// components, used to exercise the defeats (progressive = SOF2, CMYK =
    /// SOF0 with 4).
    fn jpeg_with_segment(sof_marker: u8, components: u8) -> Vec<u8> {
        let mut jpeg = vec![0xFF, 0xD8];
        let seg_len = 8 + 3 * components as usize; // length bytes + SOF dims + comps
        jpeg.extend_from_slice(&[0xFF, sof_marker]);
        jpeg.extend_from_slice(&(seg_len as u16).to_be_bytes());
        jpeg.push(8); // precision
        jpeg.extend_from_slice(&10u16.to_be_bytes()); // height
        jpeg.extend_from_slice(&12u16.to_be_bytes()); // width
        jpeg.push(components);
        for _ in 0..components {
            jpeg.extend_from_slice(&[0x01, 0x22, 0x00]);
        }
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        jpeg
    }

    #[test]
    fn jpeg_pages_embed_byte_identical() {
        let jpeg = color_jpeg_bytes();
        let mut pdf = Vec::new();
        export_pdf_pages(
            [PageInput::Jpeg(Jpeg::new(jpeg.clone()).unwrap())].into_iter(),
            &PdfOptions::default(),
            &mut pdf,
        )
        .unwrap();
        let hit = pdf.windows(jpeg.len()).any(|w| w == jpeg.as_slice());
        assert!(hit, "JPEG not embedded byte-for-byte");
        assert!(pdf.starts_with(b"%PDF") && pdf.ends_with(b"%%EOF\n"));
    }

    #[test]
    fn gray_jpeg_passthrough_uses_device_gray() {
        let inputs = vec![
            PageInput::Jpeg(Jpeg::new(gray_jpeg_bytes()).unwrap()),
            PageInput::Jpeg(Jpeg::new(color_jpeg_bytes()).unwrap()),
        ];
        let mut pdf = Vec::new();
        export_pdf_pages(inputs.into_iter(), &PdfOptions::default(), &mut pdf).unwrap();
        let s = String::from_utf8_lossy(&pdf);
        assert!(s.contains("/ColorSpace /DeviceGray"));
        assert!(s.contains("/ColorSpace /DeviceRGB"));
        assert_eq!(2, s.match_indices("/Type /Page ").count());
        assert_eq!(2 + 2 * 3 + 1, xref_in_use_objects(&pdf));
    }

    #[test]
    fn jpeg_fallback_rejects_exotic_headers() {
        // Not a JPEG at all.
        let err = Jpeg::new(b"just some bytes".to_vec()).unwrap_err();
        assert!(err.contains("baseline JPEG"), "{err}");

        // Progressive (SOF2).
        let err = Jpeg::new(jpeg_with_segment(0xC2, 3)).unwrap_err();
        assert!(err.contains("baseline JPEG"), "{err}");

        // Baseline but CMYK (4 components).
        let err = Jpeg::new(jpeg_with_segment(0xC0, 4)).unwrap_err();
        assert!(err.contains("component"), "{err}");
    }

    #[test]
    fn jpeg_side_exif_orientation_falls_back() {
        let err = Jpeg::new(with_exif_orientation(&color_jpeg_bytes(), 6)).unwrap_err();
        assert!(err.contains("orientation 6"), "{err}");

        // Orientation 1 is identity: still embeds as-is.
        let straight = with_exif_orientation(&color_jpeg_bytes(), 1);
        assert!(Jpeg::new(straight).is_ok(), "orientation 1 is not a transform");
    }

    #[test]
    fn pages_export_rejects_zero_pages() {
        let mut sink = Vec::new();
        let err = export_pdf_pages(std::iter::empty(), &PdfOptions::default(), &mut sink).unwrap_err();
        assert_eq!("no pages", err);
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
