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
