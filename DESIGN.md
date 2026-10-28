# Design

EasyScan: a very lightweight camera scanner for homework. Capture pages with the
camera, optionally auto-crop (edge detection + perspective correction) and apply
filters (e.g. B&W / grayscale / contrast), then export a multi-page PDF.
Ships on Google Play and the Apple App Store.

## Status

Empty Rust scaffold (`cargo new`). Architecture not yet chosen — pending the
stack decision below.

## Parts (planned, stack-agnostic)

1. **Capture** — camera preview, shutter, multi-page session. Platform-specific.
2. **Detect & crop** — find the page quadrilateral, let the user adjust corners,
   warp to a flat rectangle. Pure image processing; portable.
3. **Filters** — grayscale, B&W threshold (adaptive), contrast/brightness. Portable.
4. **PDF export** — pages → PDF, sensible page size and compression. Portable.
5. **Share/save** — OS share sheet / files. Platform-specific.

Parts 2–4 are the natural "core" and are good candidates for a Rust library with
unit tests on desktop; 1 and 5 need native/mobile UI.

## Risky parts

- Choosing a mobile stack that actually builds and ships to both stores
  (iOS builds need a Mac).
- Edge detection quality on real-world photos (shadows, lined paper, desks).
- Binary size ("very lightweight").
- Store requirements: camera permission strings, privacy policy, signing.

## Open questions (asked to burvy)

See DECISIONS.md once answered.
