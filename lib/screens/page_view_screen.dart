import 'dart:io';

import 'package:flutter/material.dart';

import '../src/rust/api.dart' as bridge;
import '../state/scan_session.dart';
import 'annotate_screen.dart';
import 'camera_screen.dart';
import 'crop_screen.dart';
import 'page_swipe.dart';

/// Long side of the rendered result shown here. The working copy is already
/// this size, so the result is sharp enough to judge focus at pinch zoom.
const _resultLongSide = 2000;

/// What the viewer did to the page, handed back to whoever opened it.
enum PageChange { retook, deleted }

/// One page, large: the processed result (zoomable) or the original photo
/// (zoomable), with Retake, Crop & filter, Draw and Delete underneath. Pops with a
/// [PageChange] when the page was retaken or deleted, so the caller can react.
class PageViewScreen extends StatefulWidget {
  const PageViewScreen({
    super.key,
    required this.session,
    required this.pageId,
    this.showOriginal = false,
    this.openedFromCrop = false,
  });

  final ScanSession session;
  final String pageId;

  /// Start on the original photo instead of the processed result.
  final bool showOriginal;

  /// Opened from the crop screen, which is already showing this page. Crop &
  /// filter then just goes back to it instead of stacking a second crop screen.
  final bool openedFromCrop;

  @override
  State<PageViewScreen> createState() => _PageViewScreenState();
}

class _PageViewScreenState extends State<PageViewScreen> {
  late String _pageId = widget.pageId;
  late bool _original = widget.showOriginal;
  late Future<String> _result = _renderResult();
  final _zoom = TransformationController();
  // Every render this screen made. Kept until dispose: an older one can still
  // be on screen while a newer one is being drawn.
  final List<String> _rendered = [];

  /// Result from the working copy with the page's current crop, filter and
  /// rotation. Rendered again whenever those may have changed.
  Future<String> _renderResult() async {
    final id = _pageId;
    await widget.session.whenPrepared(id);
    final page = widget.session.pageById(id);
    if (page == null) throw StateError('page was deleted');
    final out = widget.session.scratchPath('${page.id}_view${DateTime.now().microsecondsSinceEpoch}.jpg');
    await bridge.processPageFile(
      srcPath: page.workingPath!,
      corners: toBridgeCorners(page.effectiveWorkingCorners!),
      filter: toBridgeFilter(page.filter),
      rotationQuarters: 0,
      strokes: page.strokes.isEmpty ? null : toBridgeStrokeSpecs(page.strokes),
      outPath: out,
      maxLongSide: _resultLongSide,
      jpegQuality: 92,
    );
    _rendered.add(out);
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

  int get _index => widget.session.pages.indexWhere((p) => p.id == _pageId);

  /// Swiping and the arrows are off when this viewer is a detail of the crop
  /// screen, which stays on its own page.
  bool get _canPage => !widget.openedFromCrop;

  /// Moves to the next (+1) or previous (-1) page.
  void _go(int step) {
    final pages = widget.session.pages;
    final to = _index + step;
    if (!_canPage || _index < 0 || to < 0 || to >= pages.length) return;
    setState(() {
      _pageId = pages[to].id;
      _zoom.value = Matrix4.identity();
      _result = _renderResult();
    });
  }

  Future<void> _retake() async {
    final before = widget.session.pageById(_pageId);
    if (before == null) return;
    final index = _index;
    await Navigator.of(context).push<void>(MaterialPageRoute(
      builder: (_) => CameraScreen(session: widget.session, retakeIndex: index),
    ));
    if (!mounted) return;
    final after = widget.session.pageById(_pageId);
    if (after != null && after.originalPath != before.originalPath) {
      Navigator.of(context).pop(PageChange.retook);
    }
  }

  /// The crop screen may change crop, filter or rotation, so the result is
  /// rendered again when it closes.
  Future<void> _cropAndFilter() async {
    if (widget.openedFromCrop) {
      // The crop screen underneath is this page's editor already.
      Navigator.of(context).pop();
      return;
    }
    await Navigator.of(context).push<void>(MaterialPageRoute(
      builder: (_) => CropScreen(session: widget.session, pageId: _pageId, onPageChanged: (id) => _pageId = id),
    ));
    if (!mounted) return;
    setState(() => _result = _renderResult());
  }

  Future<void> _annotate() async {
    await Navigator.of(context).push<void>(MaterialPageRoute(
      builder: (_) => AnnotateScreen(session: widget.session, pageId: _pageId),
    ));
    if (!mounted) return;
    setState(() => _result = _renderResult());
  }

  Future<void> _delete() async {
    final index = widget.session.pages.indexWhere((p) => p.id == _pageId);
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
    Navigator.of(context).pop(PageChange.deleted);
    widget.session.discard(page);
  }

  @override
  void dispose() {
    for (final path in _rendered) {
      _evictAndDelete(path);
    }
    _zoom.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final index = widget.session.pages.indexWhere((p) => p.id == _pageId);
    return Scaffold(
      appBar: AppBar(
        title: Text(index < 0 ? 'Page' : 'Page ${index + 1} of ${widget.session.pages.length}'),
        actions: [
          if (_canPage) ...[
            IconButton(
              icon: const Icon(Icons.chevron_left),
              tooltip: 'Previous page',
              onPressed: index > 0 ? () => _go(-1) : null,
            ),
            IconButton(
              icon: const Icon(Icons.chevron_right),
              tooltip: 'Next page',
              onPressed: index >= 0 && index < widget.session.pages.length - 1 ? () => _go(1) : null,
            ),
          ],
        ],
      ),
      body: Column(children: [
        Expanded(
          child: PageSwipe(
            // Only when not zoomed in: a zoomed photo is panned, not paged.
            canSwipe: (_) => _canPage && _zoom.value.getMaxScaleOnAxis() < 1.05,
            onSwipe: _go,
            child: _original ? _zoomable(_originalPhoto()) : _resultView(),
          ),
        ),
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
                  child: _actionButton(_annotate, Icons.draw_outlined, 'Draw'),
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
    final page = widget.session.pageById(_pageId);
    if (page == null) return const SizedBox.shrink();
    return Image.file(File(page.originalPath), fit: BoxFit.contain, gaplessPlayback: true);
  }

  Widget _resultView() => FutureBuilder<String>(
        key: ValueKey(_pageId),
        future: _result,
        builder: (context, snap) {
          if (snap.hasError) {
            return const Center(child: Text('This photo could not be processed.'));
          }
          // Not the previous snapshot's data: it is another page's render.
          if (snap.connectionState != ConnectionState.done || !snap.hasData) {
            return const Center(child: CircularProgressIndicator());
          }
          return _zoomable(Image.file(File(snap.data!), fit: BoxFit.contain, gaplessPlayback: true));
        },
      );

  Widget _zoomable(Widget image) => InteractiveViewer(transformationController: _zoom, maxScale: 6, child: image);
}
