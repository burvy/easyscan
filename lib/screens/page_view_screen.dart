import 'dart:io';

import 'package:flutter/material.dart';

import '../src/rust/api.dart' as bridge;
import '../state/scan_session.dart';
import 'camera_screen.dart';

/// One page, large: the processed result (zoomable) or the original photo
/// (zoomable), with Retake underneath. Pops with `true` if the photo was
/// replaced, so the caller can reopen the crop view on the new photo.
class PageViewScreen extends StatefulWidget {
  const PageViewScreen({
    super.key,
    required this.session,
    required this.pageId,
    this.showOriginal = false,
  });

  final ScanSession session;
  final String pageId;

  /// Start on the original photo instead of the processed result.
  final bool showOriginal;

  @override
  State<PageViewScreen> createState() => _PageViewScreenState();
}

class _PageViewScreenState extends State<PageViewScreen> {
  late bool _original = widget.showOriginal;
  late final Future<String> _result = _renderResult();
  String? _resultPath;

  /// Full-size result from the original photo, with crop, filter and rotation.
  /// Rendered on open, so it always matches the page as it is now.
  Future<String> _renderResult() async {
    await widget.session.whenPrepared(widget.pageId);
    final page = widget.session.pageById(widget.pageId)!;
    final out = widget.session.scratchPath('${page.id}_view.jpg');
    await bridge.processPageFile(
      srcPath: page.originalPath,
      corners: toBridgeCorners(page.effectiveCorners!),
      filter: toBridgeFilter(page.filter),
      rotationQuarters: 0,
      outPath: out,
      maxLongSide: null,
      jpegQuality: 92,
    );
    _resultPath = out;
    return out;
  }

  Future<void> _retake() async {
    final before = widget.session.pageById(widget.pageId);
    if (before == null) return;
    final index = widget.session.pages.indexWhere((p) => p.id == widget.pageId);
    await Navigator.of(context).push<void>(MaterialPageRoute(
      builder: (_) => CameraScreen(session: widget.session, retakeIndex: index),
    ));
    if (!mounted) return;
    final after = widget.session.pageById(widget.pageId);
    if (after != null && after.originalPath != before.originalPath) {
      Navigator.of(context).pop(true);
    }
  }

  @override
  void dispose() {
    final path = _resultPath;
    if (path != null) {
      FileImage(File(path)).evict();
      try {
        File(path).deleteSync();
      } on FileSystemException {
        // Already gone.
      }
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final index = widget.session.pages.indexWhere((p) => p.id == widget.pageId);
    return Scaffold(
      appBar: AppBar(title: Text(index < 0 ? 'Page' : 'Page ${index + 1}')),
      body: Column(children: [
        Expanded(child: _original ? _zoomable(_originalPhoto()) : _resultView()),
        SafeArea(
          top: false,
          child: Padding(
            padding: const EdgeInsets.fromLTRB(16, 8, 16, 8),
            child: Column(mainAxisSize: MainAxisSize.min, children: [
              SizedBox(
                width: double.infinity,
                child: SegmentedButton<bool>(
                  segments: const [
                    ButtonSegment(value: false, label: Text('Result')),
                    ButtonSegment(value: true, label: Text('Original photo')),
                  ],
                  selected: {_original},
                  onSelectionChanged: (s) => setState(() => _original = s.first),
                ),
              ),
              const SizedBox(height: 8),
              SizedBox(
                width: double.infinity,
                child: OutlinedButton.icon(
                  onPressed: _retake,
                  icon: const Icon(Icons.camera_alt_outlined),
                  label: const Text('Retake'),
                ),
              ),
            ]),
          ),
        ),
      ]),
    );
  }

  Widget _originalPhoto() {
    final page = widget.session.pageById(widget.pageId);
    if (page == null) return const SizedBox.shrink();
    return Image.file(File(page.originalPath), fit: BoxFit.contain, gaplessPlayback: true);
  }

  Widget _resultView() => FutureBuilder<String>(
        future: _result,
        builder: (context, snap) {
          if (snap.hasError) {
            return const Center(child: Text('This photo could not be processed.'));
          }
          if (!snap.hasData) return const Center(child: CircularProgressIndicator());
          return _zoomable(Image.file(File(snap.data!), fit: BoxFit.contain, gaplessPlayback: true));
        },
      );

  Widget _zoomable(Widget image) => InteractiveViewer(maxScale: 6, child: image);
}
