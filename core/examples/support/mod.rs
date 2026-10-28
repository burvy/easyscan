//! Shared helpers for the real-photo examples (`measure`, `overlay`).
//!
//! Photos live in `testdata/photos/` (gitignored, private) as `<timestamp>_<page>.jpg`.
//! Ground truth is `testdata/photos/labels.tsv`: one line per photo,
//! `<file stem> x0 y0 x1 y1 x2 y2 x3 y3` with TL, TR, BR, BL as fractions of the
//! EXIF-oriented width and height, so labels survive any resize.

use std::path::{Path, PathBuf};

use easyscan_core::detect::{Point, Quad};
use image::{DynamicImage, ImageDecoder, ImageReader};

pub fn photos_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/photos")
}

/// (file stem, path) of every `*.jpg`, sorted by stem. Empty if the folder is absent.
pub fn photos() -> Vec<(String, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(photos_dir()) else {
        return vec![];
    };
    let mut v: Vec<(String, PathBuf)> = rd
        .filter_map(|e| {
            let p = e.ok()?.path();
            if p.extension()? != "jpg" {
                return None;
            }
            let n = p.file_stem()?.to_str()?.to_string();
            Some((n, p))
        })
        .collect();
    v.sort();
    v
}

pub fn load_oriented(path: &Path) -> DynamicImage {
    let mut decoder = ImageReader::open(path).unwrap().into_decoder().unwrap();
    let orientation = decoder.orientation().unwrap();
    let mut img = DynamicImage::from_decoder(decoder).unwrap();
    img.apply_orientation(orientation);
    img
}

/// Labelled quads as fractions, by file stem.
pub fn labels() -> std::collections::BTreeMap<String, [(f32, f32); 4]> {
    let mut m = std::collections::BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(photos_dir().join("labels.tsv")) else {
        return m;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut tok = line.split_whitespace();
        let Some(stem) = tok.next() else { continue };
        let f: Vec<f32> = tok.filter_map(|t| t.parse().ok()).collect();
        if f.len() == 8 {
            m.insert(
                stem.to_string(),
                [(f[0], f[1]), (f[2], f[3]), (f[4], f[5]), (f[6], f[7])],
            );
        }
    }
    m
}

pub fn to_quad(frac: &[(f32, f32); 4], w: u32, h: u32) -> Quad {
    Quad(frac.map(|(x, y)| Point {
        x: x * w as f32,
        y: y * h as f32,
    }))
}

/// Resolves a command-line photo name: an exact file stem, or a suffix such as
/// `3` when exactly one stem ends in `_3`.
pub fn find_photo(arg: &str) -> (String, PathBuf) {
    let all = photos();
    let suffix = format!("_{arg}");
    let hits: Vec<_> = all
        .iter()
        .filter(|(s, _)| s == arg || s.ends_with(&suffix))
        .collect();
    match hits.as_slice() {
        [one] => (*one).clone(),
        [] => panic!("no photo matches {arg}"),
        many => panic!(
            "{arg} is ambiguous: {:?}",
            many.iter().map(|(s, _)| s).collect::<Vec<_>>()
        ),
    }
}
