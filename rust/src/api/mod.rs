//! Thin bridge over `easyscan-core`: decode/encode at the edges, no logic.

use easyscan_core::{detect, filter, pdf};
use image::{DynamicImage, ImageFormat};
use std::io::Cursor;

#[flutter_rust_bridge::frb(init)]
pub fn init_app() {
    flutter_rust_bridge::setup_default_user_utils();
}

/// Version of the Rust core, to prove the bridge is wired up.
#[flutter_rust_bridge::frb(sync)]
pub fn core_version() -> String {
    format!("easyscan-core {}", env!("CARGO_PKG_VERSION"))
}

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

fn decode(bytes: &[u8]) -> Result<DynamicImage, String> {
    image::load_from_memory(bytes).map_err(|e| e.to_string())
}

fn encode_png(img: &DynamicImage) -> Result<Vec<u8>, String> {
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

pub fn detect_page(image: Vec<u8>) -> Result<PageDetection, String> {
    let d = detect::detect_page(&decode(&image)?);
    Ok(PageDetection {
        corners: d.quad.0.iter().map(|p| Corner { x: p.x, y: p.y }).collect(),
        found: d.found,
        confidence: d.confidence,
    })
}

/// Flattens the quad (TL, TR, BR, BL) and returns a PNG.
pub fn warp_page(image: Vec<u8>, corners: Vec<Corner>) -> Result<Vec<u8>, String> {
    let [a, b, c, d]: [Corner; 4] = corners.try_into().map_err(|_| "need 4 corners".to_string())?;
    let pt = |p: Corner| detect::Point { x: p.x, y: p.y };
    let quad = detect::Quad([pt(a), pt(b), pt(c), pt(d)]);
    encode_png(&detect::warp(&decode(&image)?, &quad))
}

/// Applies a filter and returns a PNG.
pub fn apply_filter(image: Vec<u8>, filter: ScanFilter) -> Result<Vec<u8>, String> {
    let f = match filter {
        ScanFilter::Original => filter::Filter::Original,
        ScanFilter::Grayscale => filter::Filter::Grayscale,
        ScanFilter::Enhance => filter::Filter::Enhance,
        ScanFilter::BlackWhite => filter::Filter::BlackWhite,
    };
    encode_png(&filter::apply(&decode(&image)?, f))
}

/// One PDF from ordered page images (any decodable format).
pub fn export_pdf(pages: Vec<Vec<u8>>) -> Result<Vec<u8>, String> {
    if pages.is_empty() {
        return Err("no pages".into());
    }
    let imgs = pages.iter().map(|p| decode(p)).collect::<Result<Vec<_>, _>>()?;
    Ok(pdf::export_pdf(&imgs, &pdf::PdfOptions::default()))
}
