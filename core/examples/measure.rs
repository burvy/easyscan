//! Measures page detection against hand-labelled real photos.
//!
//!   cargo run --release -p easyscan-core --example measure
//!
//! Reads `testdata/photos/*_<n>.jpg` and `testdata/photos/labels.tsv` (see
//! `support/mod.rs`); both are private and gitignored, so the example prints a
//! note and exits cleanly when they are absent. For each labelled photo it
//! prints the error of every corner as a percentage of the image diagonal
//! (TL TR BR BL), the worst of them, whether detection fell back to the full
//! frame, and the detection time (decode excluded). Pass `-v` to also print
//! the detected corners as fractions.
//!
//! Target (task 004): worst corner within 2.0% on at least 7 of 9 photos.

mod support;

use std::time::Instant;

use easyscan_core::detect::detect_page;

fn main() {
    let verbose = std::env::args().any(|a| a == "-v");
    let photos = support::photos();
    let labels = support::labels();
    if photos.is_empty() || labels.is_empty() {
        println!("no photos or labels in {}; nothing to measure", support::photos_dir().display());
        return;
    }
    println!("| photo | TL % | TR % | BR % | BL % | worst % | found | ms |");
    println!("|---|---|---|---|---|---|---|---|");
    let (mut pass, mut total) = (0, 0);
    let mut sum_worst = 0.0;
    for (n, path) in photos {
        let Some(gt) = labels.get(&n) else { continue };
        let img = support::load_oriented(&path);
        let (w, h) = (img.width(), img.height());
        let diag = ((w * w + h * h) as f32).sqrt();
        let want = support::to_quad(gt, w, h);

        let t = Instant::now();
        let det = detect_page(&img);
        let ms = t.elapsed().as_secs_f64() * 1000.0;

        let errs: Vec<f32> = det
            .quad
            .0
            .iter()
            .zip(want.0.iter())
            .map(|(a, b)| ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt() / diag * 100.0)
            .collect();
        let worst = errs.iter().cloned().fold(0.0, f32::max);
        total += 1;
        sum_worst += worst;
        if worst <= 2.0 {
            pass += 1;
        }
        println!(
            "| {n} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {} | {:.0} |",
            errs[0], errs[1], errs[2], errs[3], worst, det.found, ms
        );
        if verbose {
            let f: Vec<String> = det.quad.0.iter().map(|p| format!("({:.3},{:.3})", p.x / w as f32, p.y / h as f32)).collect();
            println!("<!-- {n}: {} -->", f.join(" "));
        }
    }
    println!("\n{pass}/{total} photos within 2.0% on every corner; mean worst-corner error {:.1}%", sum_worst / total.max(1) as f32);
}
