# Google Play Data safety answers — EasyScan (Android)

Prepared October 28, 2026, for the current app code and `google_mobile_ads` 9.2.0. The plugin includes both the legacy and Next-Gen Android SDK dependencies; the repository does not set the `USE_NEXT_GEN_SDK=true` build declaration, so these answers use Google's legacy SDK disclosure. These are answers to enter in Play Console, not a substitute for reviewing the live form when submitting. Google makes the developer responsible for accurate declarations. Re-check this document when the app or SDK changes.

## Data collection and security

- **Does the app collect or share required user data types? Yes.** The Google Mobile Ads SDK automatically transmits data off-device and shares it with Google. Collection through an SDK counts even when EasyScan's developer does not receive it.
- **Is all collected data encrypted in transit? Yes, for the data types disclosed here.** Google's current Android disclosure says Mobile Ads SDK data is encrypted in transit using TLS.
- **Can users request deletion of their data? No.** EasyScan provides no account, server, or developer-held copy of scans and no in-app deletion request mechanism. Users can delete scans locally, uninstall the app, and manage/reset the advertising ID using Android settings. This is not a mechanism to request deletion of Google-held advertising data.

## Data types to select

Declare the following data types collected and shared by the Google Mobile Ads SDK. Google Play's categories distinguish approximate from precise location; IP-derived general location is approximate. The app has no location permission, so do not declare precise location.

| Play data type | Collected? | Shared? | Required or optional? | Purpose(s) | Basis |
| --- | --- | --- | --- | --- | --- |
| Approximate location | Yes | Yes | Required (SDK/IP-based; not controlled by a location permission) | Advertising or marketing; Analytics; Fraud prevention, security, and compliance | Google says the SDK collects IP address, which may be used to estimate general location. Play says IP-inferred location belongs under approximate location. |
| App interactions | Yes | Yes | Required (SDK telemetry) | Advertising or marketing; Analytics; Fraud prevention, security, and compliance | SDK collects app launches, taps, and video views. |
| Diagnostics | Yes | Yes | Required (SDK telemetry) | Analytics; Fraud prevention, security, and compliance; Advertising or marketing | SDK collects app/SDK performance information such as launch time, hang rate, and energy usage. Google's disclosure describes these purposes collectively for its automatically collected data; select all purposes the live form permits and the current SDK data is used for. |
| Device or other IDs | Yes | Yes | Required (SDK telemetry; Android ad ID may be controlled/reset in device settings) | Advertising or marketing; Analytics; Fraud prevention, security, and compliance | SDK collects Android advertising ID, app set ID, and, if applicable, other identifiers associated with signed-in device accounts. |

**Do not select** photos, files/docs, precise location, personal info, or account management for EasyScan itself: the app processes scans locally and does not send them to the developer or an SDK. Android's share sheet can pass an exported PDF to an app the user chooses; Play excludes user-initiated sharing where the user reasonably expects the transfer. Review again if app behavior changes.

## Other form sections

- **Data deletion:** Answer **No** to whether users can request deletion. The local delete controls are not a request to delete collected data held by Google.
- **Data encryption:** Answer **Yes**, based on Google's statement that all user data collected by the Mobile Ads SDK is encrypted in transit using TLS. This answer describes SDK transfers; EasyScan does not transmit scan content.
- **Independent security review / Families commitment:** Do not claim either unless separately completed or applicable. EasyScan is intended for ages 13+ and is not a Families app.
- **App-specific account, data collection, or sharing:** None. There are no accounts or app-operated network services. Android manifest permissions are CAMERA and INTERNET. INTERNET is needed for the ad/consent SDK; app scans are local.

The Play form may change its exact wording, options, and availability. If a listed purpose is not offered for a data type, use the live form's categories and SDK guidance; do not claim a purpose not supported by the current integration. Recheck the Mobile Ads SDK disclosure for the exact version in the submitted build.

## Sources

1. Google, [Google Play data disclosure for the Google Mobile Ads SDK (Legacy, Android)](https://developers.google.com/admob/android/privacy/play-data-disclosure) — automatic collection/sharing of IP address, app interactions, diagnostics and device/account identifiers; purposes; TLS; ad ID handling. Consulted October 28, 2026. The page identifies version 25.5.0 as its latest legacy SDK at consultation; the Flutter plugin 9.2.0 changelog lists this same legacy dependency. If the build switches to Next-Gen with `USE_NEXT_GEN_SDK=true`, use [Google's Next-Gen disclosure](https://developers.google.com/admob/android/next-gen/privacy/play-data-disclosure) instead.
2. Google Play Console Help, [Provide information for Google Play's Data safety section](https://support.google.com/googleplay/android-developer/answer/10787469?hl=en) — definitions of collection, sharing, inferred approximate location, purposes, encryption and deletion questions. Consulted October 28, 2026.
3. App code: `android/app/src/main/AndroidManifest.xml` (CAMERA and INTERNET only); `lib/ads/ads.dart` and `lib/ads/privacy.dart` (UMP consent flow and privacy options); local scan/export paths in `lib/state/scan_session.dart` and `lib/state/export_service.dart`; `lib/screens/export_dialog.dart` (user-initiated share sheet).

## GitHub Pages publishing

The policy source is `docs/privacy-policy.md`, with a Jekyll front matter permalink of `/privacy-policy/`. In the public `burvy/easyscan` repository, open **Settings → Pages**, choose **Deploy from a branch**, select branch **main** and folder **/docs**, then save and wait for Pages to publish. The expected public policy URL is:

`https://burvy.github.io/easyscan/privacy-policy/`

Confirm the published URL opens before entering it in Play Console. The Pages configuration is in `docs/_config.yml`.

## AdMob `app-ads.txt` note for release

AdMob requests this exact line in `app-ads.txt`:

```text
google.com, pub-9646959833500648, DIRECT, f08c47fec0942fa0
```

The project Pages site is at `burvy.github.io/easyscan`, a project subpath, while AdMob expects the file at the root of the developer website listed in Play. Options for Burvy to decide: create a separate `burvy.github.io` user-site repository and put `app-ads.txt` at its root, then list that root website in Play; or skip `app-ads.txt` for now (ads can still serve, though publisher inventory may be marked unauthorized/unverified). Do not assume placing it under the project `/easyscan` path meets the root requirement.
