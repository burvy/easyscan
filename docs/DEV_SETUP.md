# Dev setup (Windows)

Everything is user-level (no admin) under `C:\dev`. Installed 2026-10-27.

| Piece | Version | Path |
|---|---|---|
| Flutter (stable, `git clone --depth 1 -b stable`) | 3.47.8 / Dart 3.13.7 | `C:\dev\flutter` |
| Android cmdline-tools | 15859902 (`commandlinetools-win-15859902_latest.zip`) | `C:\dev\android-sdk\cmdline-tools\latest` |
| Platform / build-tools | android-36 / 36.0.0 | `C:\dev\android-sdk` |
| NDK (Flutter's default `ndkVersion`) | 28.2.13676358 | `C:\dev\android-sdk\ndk` |
| CMake | 3.22.1 | `C:\dev\android-sdk\cmake` |
| USB driver (`extras;google;usb_driver`) | rev 13 | `C:\dev\android-sdk\extras\google\usb_driver` |
| Emulator + image | 37.2.12 / `system-images;android-35;google_apis;x86_64` | |
| AVD | `easyscan_pixel` (pixel_6) | `~\.android\avd` |
| JDK for Gradle (`flutter config --jdk-dir`) | Temurin 17.0.20 x64, zip from Adoptium | `C:\dev\jdk-17.0.20.1+1` |
| flutter_rust_bridge_codegen | 2.13.0 (`cargo install flutter_rust_bridge_codegen`) | cargo bin |
| Rust targets (nightly default toolchain) | aarch64-linux-android, armv7-linux-androideabi, x86_64-linux-android | rustup |

User environment: `ANDROID_HOME=C:\dev\android-sdk`; user `Path` gains
`C:\dev\flutter\bin`, `...\android-sdk\platform-tools`, `...\cmdline-tools\latest\bin`, `...\emulator`.

## Reproduce
```
git clone --depth 1 -b stable https://github.com/flutter/flutter.git C:\dev\flutter
# unzip cmdline-tools to C:\dev\android-sdk\cmdline-tools\latest, then:
sdkmanager --sdk_root=C:\dev\android-sdk "platform-tools" "platforms;android-36" "build-tools;36.0.0" "ndk;28.2.13676358" "emulator" "system-images;android-35;google_apis;x86_64" "cmake;3.22.1"
flutter config --android-sdk C:\dev\android-sdk
flutter doctor --android-licenses
avdmanager create avd -n easyscan_pixel -k "system-images;android-35;google_apis;x86_64" -d pixel_6
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo install flutter_rust_bridge_codegen
```

## Testing on your phone
1. Settings > About phone > tap *Build number* 7 times to unlock Developer options.
2. Settings > System > Developer options > turn on *USB debugging*.
3. Plug in with a data-capable USB cable and accept the "Allow USB debugging?" prompt (tick *Always allow*). Set USB mode to *File transfer* if nothing appears.
4. `adb devices` should list the phone as `device` (`unauthorized` = accept the prompt on the phone; empty list = driver or cable problem). `flutter devices` should show it too.
5. If Windows does not recognise it, install your brand's OEM USB driver (Samsung, Xiaomi, etc.). For Pixel/generic devices use Google's driver, already downloaded to `C:\dev\android-sdk\extras\google\usb_driver` (Device Manager > the phone > Update driver > Browse my computer > that folder; needs admin).
6. Run the app with `flutter run -d <device id>`.

The emulator stays as a secondary target once the hypervisor is enabled.

## Known issues
- `flutter doctor`: only Chrome is missing (web, not needed).
- The emulator needs a hypervisor. `emulator -accel-check` reports "hypervisor driver is not
  installed" (Windows Hypervisor Platform off, or virtualization off in BIOS). Enabling WHPX
  needs admin and a reboot. Until then, test on a physical phone with USB debugging.
- `avdmanager` prints a harmless "Could not load devices.xml" for the system image.
- Java: the pre-installed JRE 17 is 32-bit (Gradle fails on `-Xmx8G`) and lacks `jlink`, which the Android Gradle plugin needs. Use the full 64-bit JDK above: `flutter config --jdk-dir "C:\dev\jdk-17.0.20.1+1"`.
- First release build takes ~7 min (Gradle + Rust for 3 ABIs); later ones ~2 min.
