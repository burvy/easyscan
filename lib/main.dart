import 'dart:math' as math;
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';

import 'src/rust/api.dart';
import 'src/rust/frb_generated.dart';

Future<void> main() async {
  await RustLib.init();
  runApp(const EasyScanApp());
}

class EasyScanApp extends StatelessWidget {
  const EasyScanApp({super.key});

  @override
  Widget build(BuildContext context) => MaterialApp(
        title: 'EasyScan',
        theme: ThemeData(colorSchemeSeed: Colors.indigo, useMaterial3: true),
        home: const BridgeCheckScreen(),
      );
}

/// Temporary screen proving Dart -> bridge -> easyscan-core works: a synthetic
/// photo of a tilted page goes through detect, warp, filter and PDF export.
class BridgeCheckScreen extends StatefulWidget {
  const BridgeCheckScreen({super.key});

  @override
  State<BridgeCheckScreen> createState() => _BridgeCheckScreenState();
}

class _BridgeCheckScreenState extends State<BridgeCheckScreen> {
  String _status = 'Running...';
  Uint8List? _result;

  @override
  void initState() {
    super.initState();
    _run();
  }

  Future<void> _run() async {
    try {
      final photo = await _syntheticPhoto();
      final detection = await detectPage(image: photo);
      final flat = await warpPage(image: photo, corners: detection.corners);
      final bw = await applyFilter(image: flat, filter: ScanFilter.blackWhite);
      final pdf = await exportPdf(pages: [bw]);
      setState(() {
        _result = bw;
        _status = '${coreVersion()}\n'
            'page found: ${detection.found} '
            '(confidence ${detection.confidence.toStringAsFixed(2)})\n'
            'PDF: ${pdf.length} bytes';
      });
    } catch (e) {
      setState(() => _status = 'Error: $e');
    }
  }

  /// A dark desk with a light, tilted page of "text" lines, as PNG bytes.
  Future<Uint8List> _syntheticPhoto() async {
    const w = 800.0, h = 600.0;
    final rec = ui.PictureRecorder();
    final c = Canvas(rec);
    c.drawRect(const Rect.fromLTWH(0, 0, w, h), Paint()..color = const Color(0xFF2A2A2A));
    c.save();
    c.translate(w / 2, h / 2);
    c.rotate(12 * math.pi / 180);
    c.drawRect(const Rect.fromLTWH(-250, -190, 500, 380), Paint()..color = const Color(0xFFE8E4D8));
    final ink = Paint()..color = const Color(0xFF303030);
    for (var y = -150.0; y < 150; y += 26) {
      c.drawRect(Rect.fromLTWH(-210, y, 420 - (y % 52) * 2, 8), ink);
    }
    c.restore();
    final img = await rec.endRecording().toImage(w.toInt(), h.toInt());
    final data = await img.toByteData(format: ui.ImageByteFormat.png);
    return data!.buffer.asUint8List();
  }

  @override
  Widget build(BuildContext context) => Scaffold(
        appBar: AppBar(title: const Text('EasyScan')),
        body: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(children: [
            Text(_status, textAlign: TextAlign.center),
            const SizedBox(height: 16),
            if (_result != null) Expanded(child: Image.memory(_result!)),
          ]),
        ),
      );
}
