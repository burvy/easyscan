import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../models/scan_page.dart';
import '../src/rust/api.dart' as bridge;
import '../state/scan_session.dart';
import 'page_swipe.dart';
import 'page_view_screen.dart';

const _previewLongSide = 1200;
const _filterLabels = {
  PageFilter.original: 'Original',
  PageFilter.grayscale: 'Grayscale',
  PageFilter.enhance: 'Enhance',
  PageFilter.blackWhite: 'B&W',
};

/// Edit one page: drag four handles over the photo to set the crop, and pick
/// a filter with a processed preview. Edits are saved when the screen closes.
class CropScreen extends StatefulWidget {
  const CropScreen({super.key, required this.session, required this.pageId, this.onPageChanged});

  final ScanSession session;
  final String pageId;

  /// Told when swiping moves the editor to another page, so an opener that
  /// shows a page itself can follow.
  final ValueChanged<String>? onPageChanged;

  @override
  State<CropScreen> createState() => _CropScreenState();
}

class _CropScreenState extends State<CropScreen> {
  late String _pageId = widget.pageId;
  late Future<void> _ready = widget.session.whenPrepared(_pageId);

  // Working state, valid once _ready has completed.
  late ScanPage _page;
  late List<Offset> _corners;
  late PageFilter _filter;
  late int _rotation;
  late List<Offset> _initialCorners;
  // The photo this screen opened on; if it has been retaken, edits are stale.
  String? _openedPhoto;

  bool _previewing = false;
  bool _previewDirty = true;
  String? _previewPath;
  bool _rendering = false;
  int _renderSeq = 0;
  bool _loaded = false;

  void _load() {
    if (_loaded) return;
    _loaded = true;
    _page = widget.session.pageById(_pageId)!;
    _openedPhoto = _page.originalPath;
    _corners = List.of(_page.corners!);
    _initialCorners = List.of(_corners);
    _filter = _page.filter;
    _rotation = _page.rotationQuarterTurns;
  }

  Size get _imageSize => _page.imageSize!;

  bool _warnedStrokes = false;

  void _setCorners(List<Offset> c) => setState(() {
    if (!_warnedStrokes && _page.strokes.isNotEmpty) {
      _warnedStrokes = true;
      ScaffoldMessenger.of(context)
        ..clearSnackBars()
        ..showSnackBar(const SnackBar(
            content: Text('Recropping moves the page under your drawing. Your marks follow the paper; marks left outside the new crop are removed.')));
    }
        _corners = c;
        _previewDirty = true;
      });

  bool get _cropValid => isConvexQuad(_corners);

  void _showPreview() {
    if (!_cropValid) {
      ScaffoldMessenger.of(context)
        ..clearSnackBars()
        ..showSnackBar(const SnackBar(content: Text('The corners cross over. Move them to form a page outline.')));
      return;
    }
    setState(() => _previewing = true);
    if (_previewDirty) _render();
  }

  Future<void> _render() async {
    final seq = ++_renderSeq;
    setState(() {
      _rendering = true;
      _previewDirty = false;
    });
    final draft = _page.copyWith(corners: _corners, rotationQuarterTurns: _rotation);
    final out = widget.session.scratchPath('${_page.id}_p$seq.jpg');
    try {
      await bridge.processPageFile(
        srcPath: _page.workingPath!,
        corners: toBridgeCorners(draft.effectiveWorkingCorners!),
        filter: toBridgeFilter(_filter),
        rotationQuarters: 0,
        outPath: out,
        maxLongSide: _previewLongSide,
        jpegQuality: 85,
      );
    } catch (e) {
      if (mounted && seq == _renderSeq) {
        setState(() => _rendering = false);
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('Preview failed: $e')));
      }
      return;
    }
    if (!mounted || seq != _renderSeq) {
      _delete(out); // Superseded by a newer request or the screen closed.
      return;
    }
    final old = _previewPath;
    setState(() {
      _previewPath = out;
      _rendering = false;
    });
    if (old != null) {
      FileImage(File(old)).evict();
      _delete(old);
    }
  }

  void _delete(String path) {
    try {
      File(path).deleteSync();
    } on FileSystemException {
      // Scratch file, already gone is fine.
    }
  }

  void _selectFilter(PageFilter f) {
    if (!_cropValid) return _showPreview();
    setState(() {
      _filter = f;
      _previewing = true;
    });
    widget.session.setDefaultFilter(f);
    _render();
  }

  void _applyFilterToAll() {
    widget.session.applyFilterToAll(_filter);
    ScaffoldMessenger.of(context)
      ..clearSnackBars()
      ..showSnackBar(SnackBar(
        content: Text('${_filterLabels[_filter]} applied to all ${widget.session.length} pages.'),
      ));
  }

  void _rotate() {
    if (!_cropValid) return _showPreview();
    setState(() {
      _rotation = (_rotation + 1) % 4;
      _previewing = true;
    });
    _render();
  }

  /// Opens the full photo. If it is retaken there, this crop view is replaced
  /// by a fresh one so auto-detect runs on the new photo. If it is deleted
  /// there, this crop view closes too.
  Future<void> _openPhoto() async {
    final change = await Navigator.of(context).push<PageChange>(MaterialPageRoute(
      builder: (_) => PageViewScreen(
        session: widget.session,
        pageId: _pageId,
        showOriginal: true,
        openedFromCrop: true,
      ),
    ));
    if (!mounted) return;
    switch (change) {
      case PageChange.retook:
        Navigator.of(context).pushReplacement(MaterialPageRoute<void>(
          builder: (_) => CropScreen(session: widget.session, pageId: _pageId, onPageChanged: widget.onPageChanged),
        ));
      case PageChange.deleted:
        Navigator.of(context).pop();
      case null:
        break;
    }
  }

  void _commit() {
    // A retaken page has new corners of its own; do not write these over them.
    final retaken = widget.session.pageById(_pageId)?.originalPath != _openedPhoto;
    // Crossed handles would warp to garbage, so keep the last good crop.
    if (!retaken) {
      widget.session.setEdits(
        _page.id,
        corners: _cropValid ? _corners : _initialCorners,
        filter: _filter,
        rotationQuarterTurns: _rotation,
      );
    }
    _renderSeq++;
    if (_previewPath != null) {
      FileImage(File(_previewPath!)).evict();
      _delete(_previewPath!);
    }
  }

  int get _index => widget.session.pages.indexWhere((p) => p.id == _pageId);

  /// Saves this page's edits, then edits the next (+1) or previous (-1) page.
  void _go(int step) {
    final pages = widget.session.pages;
    final to = _index + step;
    if (!_loaded || _index < 0 || to < 0 || to >= pages.length) return;
    _commit();
    final id = pages[to].id;
    setState(() {
      _pageId = id;
      _loaded = false;
      _previewing = false;
      _previewDirty = true;
      _previewPath = null;
      _rendering = false;
      _ready = widget.session.whenPrepared(id);
    });
    widget.onPageChanged?.call(id);
  }

  @override
  Widget build(BuildContext context) => FutureBuilder<void>(
        future: _ready,
        builder: (context, snap) {
          if (snap.connectionState != ConnectionState.done) {
            return Scaffold(
              appBar: AppBar(title: const Text('Crop & filter')),
              body: const Center(child: CircularProgressIndicator()),
            );
          }
          if (snap.hasError || widget.session.pageById(_pageId)?.isPrepared != true) {
            return Scaffold(
              appBar: AppBar(title: const Text('Crop & filter')),
              body: Center(
                child: Column(mainAxisSize: MainAxisSize.min, children: [
                  const Text('This photo could not be processed.'),
                  const SizedBox(height: 12),
                  FilledButton(
                    onPressed: () => setState(() => _ready = widget.session.whenPrepared(_pageId)),
                    child: const Text('Try again'),
                  ),
                ]),
              ),
            );
          }
          _load();
          return _buildEditor(context);
        },
      );

  Widget _buildEditor(BuildContext context) => PopScope(
        onPopInvokedWithResult: (didPop, _) {
          if (didPop) _commit();
        },
        child: Scaffold(
          appBar: AppBar(
            title: Text('Page ${_index + 1} of ${widget.session.pages.length}'),
            actions: [
              IconButton(
                icon: const Icon(Icons.chevron_left),
                tooltip: 'Previous page',
                onPressed: _index > 0 ? () => _go(-1) : null,
              ),
              IconButton(
                icon: const Icon(Icons.chevron_right),
                tooltip: 'Next page',
                onPressed: _index >= 0 && _index < widget.session.pages.length - 1 ? () => _go(1) : null,
              ),
              IconButton(
                icon: const Icon(Icons.photo_outlined),
                tooltip: 'Full photo',
                onPressed: _openPhoto,
              ),
              TextButton(onPressed: () => Navigator.of(context).maybePop(), child: const Text('Done')),
            ],
          ),
          body: Column(children: [
            Expanded(
              child: _previewing ? PageSwipe(onSwipe: _go, child: _buildPreview()) : _buildCrop(),
            ),
            _buildControls(context),
          ]),
        ),
      );

  Widget _buildCrop() => CropEditor(
        imagePath: _page.workingPath!,
        imageSize: _imageSize,
        corners: _corners,
        valid: _cropValid,
        onChanged: _setCorners,
        onSwipe: _go,
      );

  Widget _buildPreview() => Stack(alignment: Alignment.center, children: [
        if (_previewPath != null)
          Padding(
            padding: const EdgeInsets.all(16),
            child: Image.file(File(_previewPath!), fit: BoxFit.contain, gaplessPlayback: true),
          ),
        if (_rendering) const CircularProgressIndicator(),
      ]);

  Widget _buildControls(BuildContext context) => SafeArea(
        top: false,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(8, 4, 8, 8),
          child: Column(mainAxisSize: MainAxisSize.min, children: [
            SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              child: Row(children: [
                for (final f in PageFilter.values)
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 4),
                    child: ChoiceChip(
                      label: Text(_filterLabels[f]!),
                      selected: _filter == f,
                      onSelected: (_) => _selectFilter(f),
                    ),
                  ),
              ]),
            ),
            if (widget.session.length > 1)
              Align(
                alignment: Alignment.centerRight,
                child: TextButton.icon(
                  onPressed: _applyFilterToAll,
                  icon: const Icon(Icons.done_all),
                  label: const Text('Apply to all pages'),
                ),
              ),
            const SizedBox(height: 4),
            Row(children: [
              _ToolButton(
                icon: _previewing ? Icons.crop : Icons.visibility_outlined,
                label: _previewing ? 'Edit crop' : 'Preview',
                onPressed: _previewing ? () => setState(() => _previewing = false) : _showPreview,
              ),
              _ToolButton(
                icon: Icons.auto_fix_high,
                label: 'Reset',
                onPressed: () => _resetTo(_page.detectedCorners!),
              ),
              _ToolButton(
                icon: Icons.fullscreen,
                label: 'Full image',
                onPressed: () => _resetTo(ScanSession.fullFrame(_imageSize)),
              ),
              _ToolButton(icon: Icons.rotate_right, label: 'Rotate', onPressed: _rotate),
            ]),
          ]),
        ),
      );

  void _resetTo(List<Offset> c) {
    setState(() => _previewing = false);
    _setCorners(List.of(c));
  }

  @override
  void dispose() {
    _renderSeq++; // Drop any render still in flight.
    super.dispose();
  }
}

class _ToolButton extends StatelessWidget {
  const _ToolButton({required this.icon, required this.label, required this.onPressed});

  final IconData icon;
  final String label;
  final VoidCallback onPressed;
  @override
  Widget build(BuildContext context) => Expanded(
        child: InkWell(
          onTap: onPressed,
          borderRadius: BorderRadius.circular(8),
          child: Padding(
            padding: const EdgeInsets.symmetric(vertical: 6),
            child: Column(mainAxisSize: MainAxisSize.min, children: [
              Icon(icon, color: Theme.of(context).colorScheme.primary),
              const SizedBox(height: 2),
              Text(label,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(color: Theme.of(context).colorScheme.primary)),
            ]),
          ),
        ),
      );
}

/// The photo with a draggable quad. [corners] are in [imageSize] pixels;
/// handles are kept inside the image.
class CropEditor extends StatefulWidget {
  const CropEditor({
    super.key,
    required this.imagePath,
    required this.imageSize,
    required this.corners,
    required this.valid,
    required this.onChanged,
    required this.onSwipe,
  });

  final String imagePath;
  final Size imageSize;
  final List<Offset> corners;
  final bool valid;
  final ValueChanged<List<Offset>> onChanged;

  /// A sideways swipe that did not start on a handle: +1 next, -1 previous page.
  final ValueChanged<int> onSwipe;

  @override
  State<CropEditor> createState() => _CropEditorState();
}

class _CropEditorState extends State<CropEditor> {
  static const _margin = 28.0; // Room around the photo so edge handles stay grabbable.
  static const _grabRadius = 64.0;
  static const _loupeSize = 110.0;

  int? _active;
  // A handle was picked up during the current gesture.
  bool _grabbed = false;
  Offset _grabShift = Offset.zero;

  @override
  Widget build(BuildContext context) => LayoutBuilder(builder: (context, box) {
        final avail = Size(box.maxWidth - 2 * _margin, box.maxHeight - 2 * _margin);
        final scale = math.min(avail.width / widget.imageSize.width, avail.height / widget.imageSize.height);
        final shown = Size(widget.imageSize.width * scale, widget.imageSize.height * scale);
        final origin = Offset((box.maxWidth - shown.width) / 2, (box.maxHeight - shown.height) / 2);

        Offset toWidget(Offset p) => origin + p * scale;
        Offset toImage(Offset w) => (w - origin) / scale;
        Offset clampToImage(Offset p) => Offset(
            p.dx.clamp(0.0, widget.imageSize.width), p.dy.clamp(0.0, widget.imageSize.height));

        final pts = [for (final c in widget.corners) toWidget(c)];

        void start(Offset at) {
          int? best;
          var bestDist = _grabRadius;
          for (var i = 0; i < 4; i++) {
            final d = (pts[i] - at).distance;
            if (d < bestDist) {
              best = i;
              bestDist = d;
            }
          }
          setState(() {
            _active = best;
            if (best != null) _grabbed = true;
            if (best != null) _grabShift = pts[best] - at;
          });
        }

        void move(Offset at) {
          final i = _active;
          if (i == null) return;
          final next = List.of(widget.corners);
          next[i] = clampToImage(toImage(at + _grabShift));
          widget.onChanged(next);
        }

        return PageSwipe(
          // A finger that goes down near a handle drags it; it never swipes.
          canSwipe: (at) {
            _grabbed = false;
            return pts.every((p) => (p - at).distance >= _grabRadius);
          },
          onSwipe: (step) {
            if (!_grabbed) widget.onSwipe(step);
          },
          child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onPanStart: (d) => start(d.localPosition),
          onPanUpdate: (d) => move(d.localPosition),
          onPanEnd: (_) => setState(() => _active = null),
          onPanCancel: () => setState(() => _active = null),
          child: Stack(children: [
            Positioned(
              left: origin.dx,
              top: origin.dy,
              width: shown.width,
              height: shown.height,
              // Decoded at the screen's size: the working copy is ~2000 px.
              child: Image.file(File(widget.imagePath), fit: BoxFit.fill, gaplessPlayback: true),
            ),
            Positioned.fill(
              child: CustomPaint(
                painter: _QuadPainter(pts, widget.valid ? Theme.of(context).colorScheme.primary : Colors.red, _active),
              ),
            ),
            if (_active != null) _loupe(pts[_active!], box.biggest),
          ]),
          ),
        );
      });

  /// Magnifier shown beside the dragged handle so the finger does not hide
  /// the spot being placed. It sits above the handle, or below near the top.
  Widget _loupe(Offset handle, Size area) {
    final above = handle.dy > _loupeSize + 70;
    final centre = Offset(
      handle.dx.clamp(_loupeSize / 2 + 4, area.width - _loupeSize / 2 - 4),
      above ? handle.dy - 80 : handle.dy + 80,
    );
    return Positioned(
      left: centre.dx - _loupeSize / 2,
      top: centre.dy - _loupeSize / 2,
      child: IgnorePointer(
        child: RawMagnifier(
          size: const Size.square(_loupeSize),
          magnificationScale: 2.5,
          focalPointOffset: handle - centre,
          decoration: MagnifierDecoration(
            shape: CircleBorder(side: BorderSide(color: Colors.white, width: 3)),
            shadows: const [BoxShadow(blurRadius: 6, color: Colors.black38)],
          ),
        ),
      ),
    );
  }
}

class _QuadPainter extends CustomPainter {
  _QuadPainter(this.pts, this.color, this.active);

  final List<Offset> pts;
  final Color color;
  final int? active;

  @override
  void paint(Canvas canvas, Size size) {
    final path = Path()..addPolygon(pts, true);
    canvas.drawPath(path, Paint()..color = color.withValues(alpha: 0.18));
    canvas.drawPath(
        path,
        Paint()
          ..color = color
          ..style = PaintingStyle.stroke
          ..strokeWidth = 3
          ..strokeJoin = StrokeJoin.round);
    for (var i = 0; i < 4; i++) {
      final r = i == active ? 16.0 : 13.0;
      canvas.drawCircle(pts[i], r, Paint()..color = Colors.white.withValues(alpha: 0.9));
      canvas.drawCircle(
          pts[i],
          r,
          Paint()
            ..color = color
            ..style = PaintingStyle.stroke
            ..strokeWidth = 3);
    }
  }

  @override
  bool shouldRepaint(_QuadPainter old) => old.pts != pts || old.color != color || old.active != active;
}
