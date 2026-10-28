/// The one banner EasyScan shows, docked at the bottom of the page list screen.
///
/// It reserves its own space (never covers content) and collapses to zero
/// height while there is nothing to show: offline, while ads are not yet
/// initialised/consented, or when Google fails to fill.
///
/// Size: fixed `AdSize.banner` (320x50), centred. Chosen over the adaptive
/// sizes after a device test: inline adaptive (capped at 60dp) never returned
/// a callback, while this size is reliable, non-deprecated, and the least
/// intrusive of the fixed sizes (reviewer call in 023: reliability beats full
/// width).
///
/// One bar of code on the owning screen: `bottomNavigationBar: const AdBanner()`.
library;

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:google_mobile_ads/google_mobile_ads.dart';

import 'ad_ids.dart';
import 'ads.dart';

class AdBanner extends StatefulWidget {
  const AdBanner({super.key});

  @override
  State<AdBanner> createState() => _AdBannerState();
}

class _AdBannerState extends State<AdBanner> {
  BannerAd? _ad;
  bool _loaded = false;
  bool _triedLoad = false;
  ValueListenable<bool>? _initialised;

  @override
  void initState() {
    super.initState();
    // No context use here: the kick-off lives in didChangeDependencies (and
    // this listener reacts to a late ads init after the screen is built).
    _initialised = EasyScanAds.instance.initialized..addListener(_setup);
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _setup();
  }

  @override
  void dispose() {
    _initialised?.removeListener(_setup);
    _ad?.dispose();
    super.dispose();
  }

  void _setup() {
    // Ads only exist after consent gathered + MobileAds.initialize; before
    // that, and in every offline case, staying collapsed is correct. One shot
    // per screen lifetime: no churn on every dependency change (theme, text
    // scale), no retry loops poking the ad server.
    if (_triedLoad || _ad != null || !_initialised!.value) return;
    _triedLoad = true;

    final ad = BannerAd(
      adUnitId: bannerAdUnitId,
      size: AdSize.banner,
      request: const AdRequest(),
      listener: BannerAdListener(
        onAdLoaded: (_) {
          debugPrint('[EasyScanAds] banner loaded');
          if (mounted) setState(() => _loaded = true);
        },
        onAdFailedToLoad: (failedAd, error) {
          debugPrint('[EasyScanAds] banner failed to load: $error');
          failedAd.dispose();
          // Collapsed again; the widget stays collapsed for the session.
          if (mounted) setState(() => _ad = null);
        },
      ),
    );
    if (!mounted) {
      ad.dispose();
      return;
    }
    setState(() => _ad = ad);
    debugPrint("[EasyScanAds] banner load requested (${ad.size})");
    unawaited(ad.load());
  }

  @override
  Widget build(BuildContext context) {
    final ad = _loaded ? _ad : null;
    if (ad == null) return const SizedBox.shrink();
    return SafeArea(
      top: false,
      // bottomNavigationBar reserves this space: nothing on screen is covered.
      child: Center(
        heightFactor: 1,
        child: SizedBox(
          width: ad.size.width.toDouble(),
          height: ad.size.height.toDouble(),
          child: AdWidget(ad: ad),
        ),
      ),
    );
  }
}
