import 'dart:io';

import 'package:flutter/material.dart';

import '../src/rust/api.dart' as bridge;
import '../state/scan_session.dart';
import 'camera_screen.dart';
import 'crop_screen.dart';

/// Long side of the rendered result shown here. The working copy is already
/// this size, so the result is sharp enough to judge focus at pinch zoom.
const _resultLongSide = 2000;

/// One page, large: the processed result (zoomable) or the original photo
/// (zoomable), with Retake, Crop & filter and Delete underneath. Pops with
/// `true` if the photo was replaced, so the caller can reopen the crop view
/// on the new photo.
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
  late Future<String> _result = _renderResult();
  String? _resultPath;
  int _rev = 0;

  /// Result from the working copy with the page's current crop, filter and
  /// rotation. Rendered again whenever those may have changed.
  Future<String> _renderResult() async {
    await widget.session.whenPrepared(widget.pageId);
    final page = widget.session.pageById(widget.pageId);
    if (page == null) throw StateError('page was deleted');
    final out = widget.session.scratchPath('${page.id}_view${++_rev}.jpg');
    final stale = _resultPath;
    await bridge.processPageFile(
      srcPath: page.workingPath!,
      corners: toBridgeCorners(page.effectiveWorkingCorners!),
      filter: toBridgeFilter(page.filter),
      rotationQuarters: 0,
      outPath: out,
      maxLongSide: _resultLongSide,
      jpegQuality: 92,
    );
    _resultPath = out;
    if (stale != null) _evictAndDelete(stale);
    return out;
  }

  void _evictAndDelete(String path) {
    FileImage(File(path)).evict();
    try {
      File(path).deleteSync();
    } on FileSystemException {
      // Already gone.
    }
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

  /// The crop screen may change crop, filter or rotation, so the result is
  /// rendered again when it closes.
  Future<void> _cropAndFilter() async {
    await Navigator.of(context).push<void>(MaterialPageRoute(
      builder: (_) => CropScreen(session: widget.session, pageId: widget.pageId),
    ));
    if (!mounted) return;
    setState(() => _result = _renderResult());
  }

  Future<void> _delete() async {
    final index = widget.session.pages.indexWhere((p) => p.id == widget.pageId);
    if (index < 0) return;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (dialog) => AlertDialog(
        title: const Text('Delete this page?'),
        content: const Text('The photo and its edits are removed from the scan.'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(dialog, false), child: const Text('Cancel')),
          TextButton(onPressed: () => Navigator.pop(dialog, true), child: const Text('Delete')),
        ],
      ),
    );
    if (confirmed != true || !mounted) return;
    // Take the page out before leaving so nothing still shows it, then delete
    // its files once this screen is gone.
    final page = widget.session.remove(index);
    Navigator.of(context).pop();
    widget.session.discard(page);
  }

  @override
  void dispose() {
    final path = _resultPath;
    if (path != null) _evictAndDelete(path);
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
              Row(children: [
                Expanded(
                  child: _actionButton(_retake, Icons.camera_alt_outlined, 'Retake'),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: _actionButton(_cropAndFilter, Icons.crop, 'Crop & filter'),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: _actionButton(_delete, Icons.delete_outline, 'Delete'),
                ),
              ]),
            ]),
          ),
        ),
      ]),
    );
  }

  /// One label per line: shrinks the text rather than breaking words on
  /// narrow phones.
  Widget _actionButton(VoidCallback onPressed, IconData icon, String label) => OutlinedButton.icon(
        onPressed: onPressed,
        style: OutlinedButton.styleFrom(padding: const EdgeInsets.symmetric(horizontal: 8)),
        icon: Icon(icon),
        label: FittedBox(
          fit: BoxFit.scaleDown,
          child: Text(label, maxLines: 1),
        ),
      );

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
