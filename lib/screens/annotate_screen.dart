import 'dart:async';
import 'dart:io';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';

import '../models/paper_sample.dart';
import '../models/scan_page.dart';
import '../models/strokes.dart';
import '../src/rust/api.dart' as bridge;
import '../state/scan_session.dart';

/// Long side of the result the editor draws on.
const _editLongSide = 2000;

/// Pen colours: black, navy, red, green. Curated because every one survives
/// the B&W filter's rule-line eraser (no pale blue among them).
const _penColors = [0xFF141414, 0xFF1A2F8A, 0xFFD32F2F, 0xFF2E7D32];

/// Stroke thickness as a fraction of the result's short side: S / M / L.
const _penWidths = [0.004, 0.008, 0.016];
const _whiteoutWidths = [0.02, 0.04, 0.07];

const _maxZoom = 6.0;

/// Draws on a page's result: a pen and a white-out brush, with undo/redo.
/// Strokes are only written to the page on Done.
class AnnotateScreen extends StatefulWidget {
  const AnnotateScreen({super.key, required this.session, required this.pageId});

  final ScanSession session;
  final String pageId;

  @override
  State<AnnotateScreen> createState() => _AnnotateScreenState();
}

class _AnnotateScreenState extends State<AnnotateScreen> {
  late final List<Stroke> _strokes = List.of(widget.session.pageById(widget.pageId)?.strokes ?? const []);
  late final List<Stroke> _initial;
  final List<Stroke> _redo = [];

  StrokeTool _tool = StrokeTool.pen;
  int _colorIndex = 0;
  int _sizeIndex = 1;

  // The base render (no strokes in it; they are burned in per render).
  String? _renderPath;
  List<Stroke> _rendered = const [];
  bool _rendering = false, _renderAgain = false;
  Object? _error;
  final List<String> _scratch = [];

  // View transform: screen = scale * layout + offset.
  double _scale = 1;
  Offset _offset = Offset.zero;

  // Pointers on screen, for drawing (one) and pinching (two).
  final Map<int, Offset> _pointers = {};
  List<Offset>? _live; // In-progress stroke, normalized.
  double _lastPinchDistance = 0;
  Offset _lastPinchMid = Offset.zero;

  Size _area = Size.zero;

  ui.Image? _sampleSource;
  Uint8List? _sampleRgba;

  @override
  void initState() {
    super.initState();
    _initial = List.of(_strokes);
    _render();
  }

  @override
  void dispose() {
    final path = _renderPath;
    if (path != null) _evict(path);
    for (final p in _scratch) {
      _evict(p);
    }
    _sampleSource?.dispose();
    super.dispose();
  }

  void _evict(String path) {
    FileImage(File(path)).evict();
    try {
      File(path).deleteSync();
    } on FileSystemException {
      // Already gone.
    }
  }

  /// The frame size the result is drawn at, for layout before the render lands.
  Size get _frame {
    final corners = widget.session.pageById(widget.pageId)?.effectiveWorkingCorners;
    return corners == null ? const Size(3, 4) : quadFrameSize(corners);
  }

  /// Result rect inside the (untransformed) layout area: contain-fit.
  Rect get _imageRect {
    final f = _frame;
    final fit = applyBoxFit(BoxFit.contain, f, _area);
    return Alignment.center.inscribe(fit.destination, Offset.zero & _area);
  }

  Future<void> _render() async {
    if (_rendering) {
      _renderAgain = true;
      return;
    }
    _rendering = true;
    try {
      do {
        _renderAgain = false;
        final page = widget.session.pageById(widget.pageId);
        if (page == null) return;
        await widget.session.whenPrepared(page.id);
        final strokes = List.of(_strokes);
        final out = widget.session.scratchPath('${page.id}_ann${DateTime.now().microsecondsSinceEpoch}.jpg');
        await bridge.processPageFile(
          srcPath: page.workingPath!,
          corners: toBridgeCorners(page.effectiveWorkingCorners!),
          filter: toBridgeFilter(page.filter),
          rotationQuarters: 0,
          strokes: strokes.isEmpty ? null : toBridgeStrokeSpecs(strokes),
          outPath: out,
          maxLongSide: _editLongSide,
          jpegQuality: 90,
        );
        if (!mounted) {
          _evict(out);
          return;
        }
        final old = _renderPath;
        setState(() {
          _renderPath = out;
          _rendered = strokes;
          _error = null;
        });
        _scratch.add(out);
        if (old != null) {
          _scratch.remove(old);
          _evict(old);
        }
      } while (_renderAgain && mounted);
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      _rendering = false;
    }
  }

  bool get _dirty {
    if (_strokes.length != _initial.length) return true;
    for (var i = 0; i < _strokes.length; i++) {
      if (!identical(_strokes[i], _initial[i])) return true;
    }
    return false;
  }

  void _undo() {
    if (_strokes.isEmpty) return;
    setState(() => _redo.add(_strokes.removeLast()));
    _render();
  }

  void _redoStroke() {
    if (_redo.isEmpty) return;
    setState(() => _strokes.add(_redo.removeLast()));
    _render();
  }

  // --- Pointers -----------------------------------------------------------

  Offset _toNormalized(Offset screen) {
    final layout = (screen - _offset) / _scale;
    final r = _imageRect;
    return Offset((layout.dx - r.left) / r.width, (layout.dy - r.top) / r.height);
  }

  void _down(PointerDownEvent e) {
    _pointers[e.pointer] = e.localPosition;
    if (_pointers.length == 1) {
      setState(() => _live = [_toNormalized(e.localPosition)]);
    } else {
      // A second finger means zoom: the half-drawn stroke is dropped.
      setState(() => _live = null);
      _startPinch();
    }
  }

  void _startPinch() {
    if (_pointers.length < 2) return;
    final p = _pointers.values.take(2).toList();
    _lastPinchDistance = (p[0] - p[1]).distance;
    _lastPinchMid = (p[0] + p[1]) / 2;
  }

  void _move(PointerMoveEvent e) {
    if (!_pointers.containsKey(e.pointer)) return;
    _pointers[e.pointer] = e.localPosition;
    if (_pointers.length >= 2) {
      final p = _pointers.values.take(2).toList();
      final dist = (p[0] - p[1]).distance;
      final mid = (p[0] + p[1]) / 2;
      if (_lastPinchDistance > 0) {
        final newScale = (_scale * dist / _lastPinchDistance).clamp(1.0, _maxZoom);
        final ratio = newScale / _scale;
        setState(() {
          // Keep the layout point under the midpoint fixed, then follow it.
          _offset = mid - (_lastPinchMid - _offset) * ratio;
          _scale = newScale;
          _clampView();
        });
      }
      _lastPinchDistance = dist;
      _lastPinchMid = mid;
    } else if (_live != null) {
      final n = _toNormalized(e.localPosition);
      setState(() => _live = [..._live!, n]);
    }
  }

  void _up(PointerEvent e) {
    final wasDrawing = _pointers.length == 1 && _live != null && e is PointerUpEvent;
    _pointers.remove(e.pointer);
    if (_pointers.length == 1) {
      // Back to one finger after a pinch: do not start drawing mid-gesture.
      _live = null;
    }
    if (wasDrawing) {
      _commitLive();
    } else if (e is PointerCancelEvent || _pointers.isEmpty) {
      setState(() => _live = null);
    }
    if (_pointers.length >= 2) _startPinch();
  }

  void _clampView() {
    if (_scale <= 1.0001) {
      _scale = 1;
      _offset = Offset.zero;
      return;
    }
    final minX = _area.width * (1 - _scale), minY = _area.height * (1 - _scale);
    _offset = Offset(_offset.dx.clamp(minX, 0.0), _offset.dy.clamp(minY, 0.0));
  }

  Future<void> _commitLive() async {
    final raw = _live;
    setState(() => _live = null);
    if (raw == null || raw.isEmpty) return;
    var points = decimateStrokePoints(raw, 0.002);
    if (points.length < 2) points = [points.first, points.first];
    final widths = _tool == StrokeTool.pen ? _penWidths : _whiteoutWidths;
    final width = widths[_sizeIndex];
    var color = _penColors[_penIndex];
    if (_tool == StrokeTool.whiteout) {
      color = (await _paperColor(points, width)).toARGB32();
      if (!mounted) return;
    }
    setState(() {
      _strokes.add(Stroke(tool: _tool, color: color, width: width, points: points));
      _redo.clear();
    });
    _render();
  }

  /// Paper colour around a white-out stroke, from the working copy.
  Future<Color> _paperColor(List<Offset> points, double width) async {
    final page = widget.session.pageById(widget.pageId);
    final corners = page?.effectiveWorkingCorners;
    if (page == null || corners == null || page.workingPath == null) return const Color(0xFFFFFFFF);
    try {
      if (_sampleRgba == null) {
        final bytes = await File(page.workingPath!).readAsBytes();
        final codec = await ui.instantiateImageCodec(bytes);
        final image = (await codec.getNextFrame()).image;
        final data = await image.toByteData(format: ui.ImageByteFormat.rawRgba);
        _sampleSource = image;
        _sampleRgba = data!.buffer.asUint8List();
      }
      final src = _sampleSource!;
      final size = quadFrameSize(corners);
      final thickness = width * (size.width < size.height ? size.width : size.height);
      return samplePaperColor(
        centerline: [for (final p in points) resultPointIntoPhoto(p, corners)],
        thicknessPx: thickness,
        rgba: _sampleRgba!,
        width: src.width,
        height: src.height,
      );
    } catch (_) {
      return const Color(0xFFFFFFFF);
    }
  }

  // --- UI -----------------------------------------------------------------

  Future<bool> _confirmDiscard() async {
    if (!_dirty) return true;
    final ok = await showDialog<bool>(
      context: context,
      builder: (d) => AlertDialog(
        title: const Text('Discard your drawing?'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(d, false), child: const Text('Keep drawing')),
          TextButton(onPressed: () => Navigator.pop(d, true), child: const Text('Discard')),
        ],
      ),
    );
    return ok == true;
  }

  void _done() {
    if (_dirty) widget.session.setStrokes(widget.pageId, _strokes);
    Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    return PopScope(
      canPop: false,
      onPopInvokedWithResult: (didPop, _) async {
        if (didPop) return;
        final nav = Navigator.of(context);
        if (await _confirmDiscard()) nav.pop();
      },
      child: Scaffold(
        backgroundColor: Colors.black,
        appBar: AppBar(
          title: const Text('Draw'),
          leading: IconButton(
            icon: const Icon(Icons.close),
            tooltip: 'Cancel',
            onPressed: () => Navigator.of(context).maybePop(),
          ),
          actions: [
            IconButton(
              icon: const Icon(Icons.undo),
              tooltip: 'Undo',
              onPressed: _strokes.isEmpty ? null : _undo,
            ),
            IconButton(
              icon: const Icon(Icons.redo),
              tooltip: 'Redo',
              onPressed: _redo.isEmpty ? null : _redoStroke,
            ),
            IconButton(icon: const Icon(Icons.check), tooltip: 'Done', onPressed: _done),
          ],
        ),
        body: Column(children: [
          Expanded(child: _canvas()),
          _toolbar(),
        ]),
      ),
    );
  }

  Widget _canvas() => LayoutBuilder(builder: (context, c) {
        _area = Size(c.maxWidth, c.maxHeight);
        final rect = _imageRect;
        final path = _renderPath;
        // Strokes the shown render does not contain yet are painted locally.
        final pending = [
          for (final s in _strokes)
            if (!_rendered.any((r) => identical(r, s))) s,
        ];
        return ClipRect(
          child: Listener(
            behavior: HitTestBehavior.opaque,
            onPointerDown: _down,
            onPointerMove: _move,
            onPointerUp: _up,
            onPointerCancel: _up,
            child: Stack(children: [
              Positioned.fill(
                child: Transform(
                  transform: Matrix4.translationValues(_offset.dx, _offset.dy, 0)
                    ..scaleByDouble(_scale, _scale, 1, 1),
                  child: Stack(children: [
                    if (path != null)
                      Positioned.fromRect(
                        rect: rect,
                        child: Image.file(File(path), fit: BoxFit.fill, gaplessPlayback: true),
                      ),
                    Positioned.fromRect(
                      rect: rect,
                      child: CustomPaint(
                        painter: _StrokePainter(pending, _live, _tool, _penColors[_penIndex],
                            (_tool == StrokeTool.pen ? _penWidths : _whiteoutWidths)[_sizeIndex]),
                      ),
                    ),
                  ]),
                ),
              ),
              if (path == null)
                Center(
                  child: _error != null
                      ? const Text('This photo could not be processed.', style: TextStyle(color: Colors.white))
                      : const CircularProgressIndicator(),
                ),
            ]),
          ),
        );
      });

  bool get _blackAndWhite => widget.session.pageById(widget.pageId)?.filter == PageFilter.blackWhite;

  /// B&W burns every pen colour to black, so the pen is forced to black there.
  int get _penIndex => _blackAndWhite ? 0 : _colorIndex;

  Widget _toolbar() => ColoredBox(
        color: Theme.of(context).colorScheme.surface,
        child: SafeArea(
        top: false,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(8, 8, 8, 8),
          child: Column(mainAxisSize: MainAxisSize.min, children: [
            SegmentedButton<StrokeTool>(
              segments: const [
                ButtonSegment(value: StrokeTool.pen, icon: Icon(Icons.edit), label: Text('Pen')),
                ButtonSegment(value: StrokeTool.whiteout, icon: Icon(Icons.format_paint), label: Text('White-out')),
              ],
              selected: {_tool},
              onSelectionChanged: (s) => setState(() => _tool = s.first),
            ),
            if (_tool == StrokeTool.pen && _blackAndWhite)
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Text('Black & white pages take a black pen. Colours show with Original or Enhanced.',
                    textAlign: TextAlign.center,
                    style: TextStyle(fontSize: 12, color: Theme.of(context).colorScheme.onSurfaceVariant)),
              ),
            const SizedBox(height: 8),
            Row(mainAxisAlignment: MainAxisAlignment.spaceEvenly, children: [
              if (_tool == StrokeTool.pen && !_blackAndWhite)
                for (var i = 0; i < _penColors.length; i++) _colorDot(i),
              if (_tool == StrokeTool.pen && !_blackAndWhite) const SizedBox(width: 12),
              for (var i = 0; i < 3; i++) _sizeDot(i),
            ]),
          ]),
        ),
        ),
      );

  Widget _colorDot(int i) => GestureDetector(
        onTap: () => setState(() => _colorIndex = i),
        child: Container(
          width: 40,
          height: 40,
          decoration: BoxDecoration(
            color: Color(_penColors[i]),
            shape: BoxShape.circle,
            border: Border.all(color: i == _colorIndex ? Colors.black87 : Colors.black26, width: i == _colorIndex ? 3 : 1),
          ),
        ),
      );

  Widget _sizeDot(int i) {
    final selected = i == _sizeIndex;
    return GestureDetector(
      onTap: () => setState(() => _sizeIndex = i),
      child: Container(
        width: 44,
        height: 44,
        alignment: Alignment.center,
        decoration: BoxDecoration(
          shape: BoxShape.circle,
          border: Border.all(color: selected ? Colors.black87 : Colors.black26, width: selected ? 3 : 1),
        ),
        child: Container(
          width: 6.0 + i * 6,
          height: 6.0 + i * 6,
          decoration: const BoxDecoration(color: Colors.black54, shape: BoxShape.circle),
        ),
      ),
    );
  }
}

/// Paints strokes in the result rect: committed ones the render does not
/// show yet, and the in-progress one.
class _StrokePainter extends CustomPainter {
  _StrokePainter(this.strokes, this.live, this.tool, this.penColor, this.liveWidth);

  final List<Stroke> strokes;
  final List<Offset>? live;
  final StrokeTool tool;
  final int penColor;
  final double liveWidth;

  void _line(Canvas canvas, Size size, List<Offset> pts, Color color, double width) {
    final thickness = width * (size.width < size.height ? size.width : size.height);
    final paint = Paint()
      ..color = color
      ..style = PaintingStyle.stroke
      ..strokeWidth = thickness
      ..strokeCap = StrokeCap.round
      ..strokeJoin = StrokeJoin.round
      ..isAntiAlias = true;
    final path = Path();
    for (var i = 0; i < pts.length; i++) {
      final p = Offset(pts[i].dx * size.width, pts[i].dy * size.height);
      i == 0 ? path.moveTo(p.dx, p.dy) : path.lineTo(p.dx, p.dy);
    }
    if (pts.length == 1) {
      canvas.drawCircle(Offset(pts[0].dx * size.width, pts[0].dy * size.height), thickness / 2,
          paint..style = PaintingStyle.fill);
    } else {
      canvas.drawPath(path, paint);
    }
  }

  @override
  void paint(Canvas canvas, Size size) {
    canvas.clipRect(Offset.zero & size);
    for (final s in strokes) {
      _line(canvas, size, s.points, Color(s.color), s.width);
    }
    final l = live;
    if (l != null && l.isNotEmpty) {
      // White-out previews as translucent paper; the real colour is sampled
      // when the stroke is committed.
      final color = tool == StrokeTool.pen ? Color(penColor) : const Color(0xCCFFFFFF);
      _line(canvas, size, l, color, liveWidth);
    }
  }

  @override
  bool shouldRepaint(_StrokePainter old) => true;
}
