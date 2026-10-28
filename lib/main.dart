import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'ads/ads_feature.dart';
import 'screens/page_list_screen.dart';
import 'src/rust/frb_generated.dart';
import 'state/scan_session.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await SystemChrome.setPreferredOrientations([DeviceOrientation.portraitUp]);
  await RustLib.init();
  final session = await ScanSession.create();
  runApp(EasyScanApp(session: session));
  // Ads are not on the critical path: UMP consent and ad init run on their own
  // after startup, and offline they simply fizzle (no ads). They may show a
  // consent form on first run in the EEA/UK.
  unawaited(Future(() async {
    await EasyScanAds.instance.initialize();
  }));
}

class EasyScanApp extends StatelessWidget {
  const EasyScanApp({super.key, required this.session});

  final ScanSession session;

  @override
  Widget build(BuildContext context) => MaterialApp(
        title: 'EasyScan',
        theme: ThemeData(colorSchemeSeed: Colors.indigo, useMaterial3: true),
        home: PageListScreen(session: session),
      );
}
