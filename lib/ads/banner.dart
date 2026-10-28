/// The one banner EasyScan shows: an anchored adaptive banner docked at the
/// bottom of the page list screen.
///
/// It reserves its own space (never covers content) and collapses to zero
/// height while there is nothing to show: offline, while ads are not yet
/// initialised/consented, or when Google fails to fill.
///
/// One linear bar of code on the owning screen: `bottomNavigationBar: AdBanner()`.
library;

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
  ValueListenable<bool>? _initialised;

  @override
  void initState() {
    super.initState();
    _initialised = EasyScanAds.instance.initialized..addListener(_setup);
    _setup();
  }

  void _setup() async {
    // Ads only exist after consent gathered + MobileAds.initialize; before
    // that, and in every offline case, staying collapsed is correct.
    if (!_initialised!.value || _ad != null) return;

    final width = MediaQuery.sizeOf(context).width.truncate();
    // Adaptive: Google pick a height in 50-150dp, never more than 20% of the
    // screen. (The older plain-anchored call is deprecated; this large one is
    // its replacement and still just a bottom banner.)
    final size = await AdSize.getLargeAnchoredAdaptiveBannerAdSize(width);
    if (size == null) return;

    final ad = BannerAd(
      adUnitId: bannerAdUnitId,
      size: size,
      request: const AdRequest(),
      listener: BannerAdListener(
        onAdLoaded: (_) {
          if (mounted) setState(() => _loaded = true);
        },
        onAdFailedToLoad: (ad, _) {
          ad.dispose();
          if (mounted) setState(() => _ad = null); // back to zero height
        },
      ),
    );
    if (!mounted) {
      ad.dispose();
      return;
    }
    setState(() => _ad = ad);
    await ad.load();
  }

  @override
  void dispose() {
    _initialised?.removeListener(_setup);
    _ad?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final ad = _loaded ? _ad : null;
    if (ad == null) return const SizedBox.shrink();
    return SafeArea(
      top: false,
      child: SizedBox(
        width: ad.size.width.toDouble(),
        height: ad.size.height.toDouble(),
        child: AdWidget(ad: ad),
      ),
    );
  }
}
