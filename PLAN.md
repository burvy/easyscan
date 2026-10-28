# Plan

## Focus: G1 core — Rust image pipeline (`core/`)
- PDF export (pdf.rs)
- Filters (filter.rs)
- Page detection + perspective warp (detect.rs)

## Next groups
- G2 toolchain — install Flutter + Android SDK, `flutter create`, wire flutter_rust_bridge to core, run on a device.
- G3 app — camera capture, page list with reorder/retake, crop handles, filter picker, export + share.
- G4 ads — single AdMob banner + UMP consent.
- G5 release — app icon/name, signing, privacy policy, Play Console walkthrough with burvy.
