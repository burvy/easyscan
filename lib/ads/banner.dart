/// The one banner EasyScan shows, docked at the bottom of the page list screen.
///
/// It reserves its own space (never covers content) and collapses to zero
/// height while there is nothing to show: offline, while ads are not yet
/// initialised/consented, or when Google fails to fill. On fill, an inline
/// adaptive banner sized for the screen width is shown, capped at 60dp so it
/// stays unobtrusive (Google's minimum banner height applies, 50dp).
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
  /// The unobtrusive ceiling for this app's single banner.
  static const int _maxHeightDp = 60;

  BannerAd? _ad;
  AdSize? _platformSize;
  ValueListenable<bool>? _initialised;
  bool _triedLoad = false;

  @override
  void initState() {
    super.initState();
    // No context / MediaQuery use here: mounting the screen happens before
    // and after ads may already be initialised, so the kick-off lives in
    // didChangeDependencies (and this listener reacts to late init).
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
    // per screen lifetime: no churn on every dependency change (e.g. theme,
    // text scale), no vendored retry loops poking the ad server.
    if (_triedLoad || _ad != null || !_initialised!.value) return;
    _triedLoad = true;

    final width = MediaQuery.sizeOf(context).width.truncate();
    final ad = BannerAd(
      // Inline adaptive: Google picks a height between its 50dp floor and
      // [_maxHeightDp]; the real height is only known after the load.
      size: AdSize.getInlineAdaptiveBannerAdSize(width, _maxHeightDp),
      adUnitId: bannerAdUnitId,
      request: const AdRequest(),
      listener: BannerAdListener(
        onAdLoaded: (loadedAd) async {
          final size = await (loadedAd as BannerAd).getPlatformAdSize();
          if (!mounted) return;
          setState(() {
            _platformSize = size ?? AdSize(width: width, height: _maxHeightDp);
          });
        },
        onAdFailedToLoad: (failedAd, error) {
          debugPrint('[EasyScanAds] banner failed to load: $error');
          failedAd.dispose();
          // Collapsed again; the widget stays collapsed for the session.
          if (mounted) {
            setState(() {
              _ad = null;
              _platformSize = null;
            });
          }
        },
      ),
    );
    if (!mounted) {
      ad.dispose();
      return;
    }
    setState(() => _ad = ad);
    unawaited(ad.load());
  }

  @override
  Widget build(BuildContext context) {
    final size = _platformSize;
    final ad = _ad;
    // InlineSize has height 0 until the platform reports the real one.
    if (ad == null || size == null) return const SizedBox.shrink();
    return SafeArea(
      top: false,
      // bottomNavigationBar reserves this space: nothing on screen is covered.
      child: SizedBox(
        width: size.width.toDouble(),
        height: size.height.toDouble(),
        child: AdWidget(ad: ad),
      ),
    );
  }
}
