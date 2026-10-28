/// UMP consent gathering and one-time init of the Google Mobile Ads SDK.
///
/// Google requires an EEA/UK user's consent before personalised ads; the UMP
/// SDK shows the right form for the user's region (or nothing where none is
/// needed). Ads are only ever initialised after consent information has been
/// fully gathered, and the SDK itself refuses ads until the user has consented
/// (or consent is not required), so a user who declines never sees ads.
library;

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:google_mobile_ads/google_mobile_ads.dart';

import 'ad_ids.dart';

/// Maximum ad content rating: the app is 13+, NOT in the Families program.
/// `tagForUnderAgeOfConsent` is deliberately left at its default (not set).
final String _maxAdContentRating = MaxAdContentRating.t;

/// True in debug builds so the Pixel always simulates the EEA consent flow.
/// Debug geography is honoured only on the debug devices listed below; in
/// release builds the constant body of these lists is empty and the real
/// user's region decides. (With only the debug geography set, UMP logs
/// "Use new ConsentDebugSettings... to set this as a debug device".)
const bool _eeaDebugEnabled = kDebugMode;
const String _eeaDebugDeviceId = '75F0DC1FCB0512FFED5A03A125A06B16'; // burvy's Pixel 9
const List<String>? _eeaDebugDevices = _eeaDebugEnabled ? [_eeaDebugDeviceId] : null;

/// Tracks the consent + ads boot sequence. `main.dart` kicks it off after the
/// first frame so UI startup never waits on it; widgets in this library watch
/// [initialized] and [privacyOptionsRequired] instead of polling.
class EasyScanAds {
  EasyScanAds._();

  static final EasyScanAds instance = EasyScanAds._();

  final ValueNotifier<bool> _initialized = ValueNotifier<bool>(false);
  final ValueNotifier<bool> _privacyOptionsRequired = ValueNotifier<bool>(false);

  /// Whether the consent flow finished and `MobileAds.initialize` was called
  /// (i.e. the banner may start loading). Never re-runs the init.
  ValueListenable<bool> get initialized => _initialized;

  /// Whether Google currently makes us show a "Privacy options" entry point.
  /// The app bar's privacy button is only shown while this is true.
  ValueListenable<bool> get privacyOptionsRequired => _privacyOptionsRequired;

  void _initializeAds() {
    if (_initialized.value) return;
    _initialized.value = true;
    // Setting the configuration before initialize() makes sure the very first
    // ad requests are already T-rated for our 13+ audience.
    MobileAds.instance
        .updateRequestConfiguration(
          RequestConfiguration(maxAdContentRating: _maxAdContentRating),
        )
        .ignore();
    // Ads init must not be awaited by the UI; late init is harmless and the
    // ad loader below waits for MobileAds.initialize anyway.
    MobileAds.instance.initialize();
    if (kDebugMode) unawaited(_debugSmokeTest());
  }

  /// Debug-only double check that consent + IDs really fill a banner request
  /// (no UI involved); in release this whole method never runs.
  Future<void> _debugSmokeTest() async {
    final ad = BannerAd(
      adUnitId: bannerAdUnitId,
      size: AdSize.banner,
      request: const AdRequest(),
      listener: BannerAdListener(
        onAdLoaded: (_) => debugPrint('[EasyScanAds] smoke test banner loaded ✓'),
        onAdFailedToLoad: (_, error) =>
            debugPrint('[EasyScanAds] smoke test banner FAILED: $error'),
      ),
    );
    await ad.load();
    // Platform view already released to its listener's owner: the smoke
    // banner has no UI, so free it well before any screen could show a real ad.
    Timer(const Duration(seconds: 10), () {
      ad.dispose();
      debugPrint('[EasyScanAds] smoke test banner disposed');
    });
  }

  /// Gathers consent (UMP), then initialises the ad SDK if ads are allowed.
  ///
  /// Errors e.g. when offline are swallowed: consent gathering is re-offered
  /// next launch and until `canRequestAds()` is true no ad request is made.
  Future<void> initialize() async {
    final consent = ConsentInformation.instance;

    // A completed consent from a previous session acts immediately, and a
    // fresh "not obtained yet" status refreshes via the network below (the
    // Google-recommended double path).
    if (await consent.canRequestAds()) _initializeAds();

    final params = ConsentRequestParameters(
      consentDebugSettings: _eeaDebugEnabled
          ? ConsentDebugSettings(
              debugGeography: DebugGeography.debugGeographyEea,
              testIdentifiers: _eeaDebugDevices,
            )
          : null,
    );

    final gather = Completer<void>();
    consent.requestConsentInfoUpdate(
      params,
      gather.complete,
      (_) => gather.complete(), // Errors resolve the same way; no ads offline.
    );
    await gather.future;

    if (await consent.isConsentFormAvailable()) {
      // Shows the EEA/UK consent form when one is required, right at startup.
      await ConsentForm.loadAndShowConsentFormIfRequired((_) {});
    }

    if (await consent.canRequestAds()) _initializeAds();

    await consent
        .getPrivacyOptionsRequirementStatus()
        .then((s) => _privacyOptionsRequired.value =
            s == PrivacyOptionsRequirementStatus.required);
  }

  /// Opens the privacy options form (a required entry point in the EEA/UK).
  ///
  /// The app bar only shows the button when [privacyOptionsRequired] is true;
  /// double safety on top of Google's own error handling.
  Future<void> showPrivacyOptions() => ConsentForm.showPrivacyOptionsForm((_) {});
}
