# Decisions

One line per decision, with the reason.

- 2026-10-27 Stack: Flutter UI + Rust core via flutter_rust_bridge. One UI codebase, image pipeline testable in Rust. (burvy accepted recommendation)
- 2026-10-27 Android/Google Play only; no iOS. (burvy)
- 2026-10-27 Offline, no accounts; one non-intrusive banner ad (AdMob). (burvy)
- 2026-10-27 v1 scope: scan, reorder/retake pages, auto-crop with manual adjust, filters, PDF, share. (burvy)
- 2026-10-27 Core lives in `core/` as a pure lib crate (workspace member) with no FFI; the FRB bridge crate is a separate thin layer. Keeps core testable on desktop and independent of codegen.
- 2026-10-27 No OpenCV; use `image`/`imageproc`-level crates. Keeps the APK small and the build simple.
- 2026-10-27 Target audience 13+ (not Families program); keep one AdMob banner with UMP consent, declared in Data safety. App works offline; ads load when online. (burvy, ask 002)
- 2026-10-27 An agent installs Flutter + Android SDK/NDK on the dev PC. (burvy, ask 002)
- 2026-10-27 Testing on emulator for dev plus burvy's real Android phone over USB debugging for camera/crop/filter tuning. (burvy, ask 003)
- 2026-10-28 Bridge passes file paths, not image bytes, for full-res work (camera JPEG in, processed JPEG/PDF out). 12MP PNG round-trips through FFI are slow and double memory. Byte APIs stay only for small previews.
- 2026-10-28 Page model lives in Dart: original photo path + corners + filter (+ rotation). Processing is re-run from the original, so edits are non-destructive and retakes just replace the original.
- 2026-10-28: B&W drops blue ruled lines entirely (keep only writing); Enhance stays natural with lines. Reason: burvy prefers clean writing for teachers; dashed lines were worst of both. (014)
- 2026-10-28 The in-progress scan persists across app restarts (one scan, not history); cleared only by an explicit New scan. Reason: Android kills backgrounded apps, and a lost multi-page scan is the worst failure for a student. (015)
- 2026-10-28 G3b polish (016-020) goes before G4 ads. (burvy, ask 014)
- 2026-10-28 Past scans: keep finished scans and allow reopening them for editing (not just a PDF list). Supersedes the one-scan rule from 015. (burvy, ask 014; 021)
- 2026-10-28 Drawing exists to change answers on a page (cover old writing, write new), burned into the PDF. (burvy, ask 014; 022)
- 2026-10-28 Scan history: every scan auto-kept in scans/<id>/ (own state.json), no auto-eviction (size shown, manual delete), New scan without confirm. Working copies dropped on switch, rebuilt keeping corners. (021 design, Sonnet)
