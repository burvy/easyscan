# Releasing easyscan

Play App Signing is used: Google holds the app signing key, and you hold only
an **upload key** (resettable through Play support if you lose it). The upload
key never goes into git; `android/key.properties`, `*.jks` and `*.keystore` are
all gitignored.

## One-time: create the upload key

Run this yourself (not an agent), anywhere outside the repo, e.g. a `keys`
folder in your documents:

```
keytool -genkeypair -v -keystore upload-keystore.jks -alias upload ^
  -keyalg RSA -keysize 2048 -validity 10000
```

(`keytool` is in the JDK's `bin`, e.g. `C:\dev\jdk-17.0.20.1+1\bin`.) It asks for
a store password, a key password and your name details. Pick strong passwords
and store them in your password manager.

**Back it up** (the `.jks` file and both passwords) in at least two places, such
as a password manager attachment plus an encrypted USB or cloud vault. If it is
lost you can still ship, but only after asking Play support to reset the upload
key.

Then create `android/key.properties` (gitignored):

```
storePassword=<store password>
keyPassword=<key password>
keyAlias=upload
storeFile=C:/path/to/upload-keystore.jks
```

`storeFile` may be an absolute path (preferred) or relative to `android/`.

## Each release

1. Bump `version:` in `pubspec.yaml`. The part before `+` is the version name
   shown to users (`1.0.1`); the number after `+` is the version code and
   **must increase with every upload** to Play (`1.0.1+2`).
2. Build the bundle:
   ```
   flutter build appbundle --release
   ```
   Output: `build/app/outputs/bundle/release/app-release.aab`. Upload that in the
   Play Console.
3. To sideload-test the same build on a phone:
   ```
   flutter build apk --release --target-platform android-arm64
   adb install -r build/app/outputs/flutter-apk/app-release.apk
   ```
   Release builds request the real AdMob banner unit. Never tap the ad; use a
   device registered as an AdMob test device.

A release build without `android/key.properties` **fails** with a readable
message; it never falls back to the debug key. (Debug builds and
`flutter run` are unaffected.)

## R8 / minification

Release builds run R8 with code and resource shrinking
(`android/app/proguard-rules.pro`). flutter_rust_bridge uses `dart:ffi`, so there
is no Java to keep for it, and the AdMob/UMP SDKs ship their own consumer rules.
If a release-only crash appears, look at `build/app/outputs/mapping/release/` and
add `-keep` rules to `proguard-rules.pro`.
