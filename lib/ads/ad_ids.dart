// All AdMob IDs live here (plus the app ID in the Android manifest) so the
// release swap is one edit per ID.
//
// TODO(release): replace these Google test IDs with burvy's real AdMob IDs
// before publishing. Test IDs never pay out but always fill, so they are safe
// to keep until release: https://developers.google.com/admob/android/test-ads

/// The AdMob application ID.
///
/// Mirrored in `android/app/src/main/AndroidManifest.xml` as the
/// `com.google.android.gms.ads.APPLICATION_ID` meta-data value (the Android SDK
/// reads it from the manifest only). Both copies must be swapped together.
const String adMobAppId = 'ca-app-pub-3940256099942544~3347511713';

/// The banner ad unit ID, shown at the bottom of the page list screen.
const String bannerAdUnitId = 'ca-app-pub-3940256099942544/6300978111';
