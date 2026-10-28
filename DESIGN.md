# Design

EasyScan: a very lightweight camera scanner for homework. Capture pages with the
camera, reorder/retake them, optionally auto-crop (edge detection + perspective
correction) and apply filters, then export one PDF and share it.
Android only (Google Play). Offline, no accounts; one non-intrusive banner ad.

## Architecture

```
Flutter app (Dart UI, repo root, later)      Android only
  camera · page list (reorder/retake) · crop handles · filter picker · share · ad banner
        │  flutter_rust_bridge (thin `rust/` bridge crate, later)
        ▼
core/  easyscan-core (pure Rust lib, tested with `cargo test` on desktop)
  detect.rs  find page quad in a photo; warp quad -> flat rectangle
  filter.rs  grayscale, B&W (adaptive threshold), contrast/auto-enhance
  pdf.rs     pages (images) -> one PDF
```

- **core has no platform or FFI code.** It takes and returns plain types
  (image buffers, corner points, bytes) so it is unit-testable on Windows and the
  bridge layer stays trivial.
- **The UI owns the session** (list of pages, order, per-page corners and filter).
  Rust functions are stateless: photo + corners + filter in, processed image out;
  page images in, PDF bytes out.
- Detection only *suggests* corners; the user can always drag them. A bad
  detection must degrade to "full image", never to an error.

## Risky parts

- Detection quality on real photos (shadows, lined paper, busy desks).
- App size: keep Rust deps lean (`image` with only needed codecs, no OpenCV).
- Memory: phone photos are 12+ MP; downscale for detection, warp at full res
  only on export.
- Ads: AdMob needs INTERNET, a consent form (UMP) in the EU/UK, and must be
  declared in Play's Data safety form. If the audience includes under-13s, Play's
  Families policy applies.
- Toolchain: Flutter + Android SDK are not yet installed on the dev machine.
