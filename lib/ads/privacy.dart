/// The entry point Google's UMP rules require wherever consent was gathered:
/// an app bar action that opens the "Privacy options" form (EEA/UK).
///
/// Invisible until Google actually requires it, and the owning screen adds it
/// with one line: `actions: [..., AdPrivacyAction()]`.
library;

import 'package:flutter/material.dart';

import 'ads.dart' show EasyScanAds;

class AdPrivacyAction extends StatelessWidget {
  const AdPrivacyAction({super.key});

  @override
  Widget build(BuildContext context) => ValueListenableBuilder<bool>(
        valueListenable: EasyScanAds.instance.privacyOptionsRequired,
        builder: (context, required, _) => required
            ? IconButton(
                icon: const Icon(Icons.privacy_tip_outlined),
                tooltip: 'Privacy options',
                onPressed: EasyScanAds.instance.showPrivacyOptions,
              )
            : const SizedBox.shrink(),
      );
}
