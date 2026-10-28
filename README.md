# EasyScan

Lightweight Android homework scanner: camera -> optional auto-crop -> filters -> one PDF.
Flutter UI over a pure-Rust core (`core/`), connected by flutter_rust_bridge (`rust/`).
See `DESIGN.md` for the architecture and `docs/DEV_SETUP.md` for the toolchain.

## Layout
- `core/` easyscan-core: detection/warp, filters, PDF. Plain Rust, `cargo test`.
- `rust/` easyscan_bridge: thin FRB wrapper (decode/encode + calls into core).
- `rust_builder/` cargokit plugin that builds `rust/` during the Android build.
- `lib/` Dart app; `lib/src/rust/` is generated, do not edit.

## Build and run
Prerequisites: see `docs/DEV_SETUP.md` (Flutter, Android SDK/NDK, Rust Android targets).

```
cargo test --workspace                       # core + bridge
flutter run                                  # phone (USB debugging) or emulator
flutter build apk --release --split-per-abi  # build/app/outputs/flutter-apk/
```

After changing `rust/src/api/`, regenerate the Dart bindings:

```
flutter_rust_bridge_codegen generate
```
