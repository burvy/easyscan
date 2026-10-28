// All AdMob IDs live here (plus the app ID in the Android manifest) so the
// release swap is one edit per ID.

import 'package:flutter/foundation.dart';

/// The AdMob application ID.
///
/// Mirrored in `android/app/src/main/AndroidManifest.xml` as the
/// `com.google.android.gms.ads.APPLICATION_ID` meta-data value (the Android SDK
/// reads it from the manifest only). Both copies must be swapped together.
/// Unlike unit IDs, the app ID is safe to use in debug builds too — the ban
/// risk is in loading real ads, which never happens below.
const String adMobAppId = 'ca-app-pub-3940256099942544~3347511713';

/// Google's dedicated banner test unit: always fills, never pays out.
/// https://developers.google.com/admob/android/test-ads
const String _googleTestBannerUnitId =
    'ca-app-pub-3940256099942544/6300978111';

/// TODO(release): replace with burvy's real banner ad unit ID once created in
/// the AdMob account.
const String _realBannerUnitId = _googleTestBannerUnitId;

/// The banner ad unit ID, shown at the bottom of the page list screen.
///
/// Debug builds are pinned to Google's test unit so nobody on the dev side can
/// ever load or click a real ad (an AdMob account-ban risk); only release
/// builds request the real unit.
const String bannerAdUnitId = kDebugMode ? _googleTestBannerUnitId : _realBannerUnitId;
