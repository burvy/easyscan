import 'dart:async';
import 'dart:io';
import 'dart:ui';

import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';

import '../models/scan_page.dart';
import '../src/rust/api.dart' as bridge;

/// Long side of the per-page working copy that previews run on.
const _workingLongSide = 2000;
const _thumbLongSide = 240;

bridge.ScanFilter toBridgeFilter(PageFilter f) => switch (f) {
      PageFilter.original => bridge.ScanFilter.original,
      PageFilter.grayscale => bridge.ScanFilter.grayscale,
      PageFilter.enhance => bridge.ScanFilter.enhance,
      PageFilter.blackWhite => bridge.ScanFilter.blackWhite,
    };

List<bridge.Corner> toBridgeCorners(List<Offset> c) =>
    [for (final p in c) bridge.Corner(x: p.dx, y: p.dy)];

/// The pages of the current scan, in export order. Lives for the app session
/// only (no history across launches); photos sit in private app storage.
class ScanSession extends ChangeNotifier {
  ScanSession._(this._dir);

  final Directory _dir;
  final List<ScanPage> _pages = [];
  int _counter = 0;

  List<ScanPage> get pages => List.unmodifiable(_pages);
  int get length => _pages.length;

  /// Creates the session and clears photos left over from a previous run.
  static Future<ScanSession> create() async {
    final root = await getApplicationDocumentsDirectory();
    final dir = Directory('${root.path}/pages');
    if (dir.existsSync()) dir.deleteSync(recursive: true);
    dir.createSync(recursive: true);
    return ScanSession._(dir);
  }

  /// Moves a freshly captured photo into private storage under a new,
  /// never-reused name (so a retake cannot show a stale cached thumbnail).
  Future<ScanPage> _adopt(String capturedPath) async {
    final id = '${DateTime.now().microsecondsSinceEpoch}_${_counter++}';
    final dest = '${_dir.path}/$id.jpg';
    await File(capturedPath).copy(dest);
    try {
      await File(capturedPath).delete();
    } on FileSystemException {
      // The camera's cache file is disposable anyway.
    }
    return ScanPage(id: id, originalPath: dest);
  }

  Future<void> addPhoto(String capturedPath) async {
    final page = await _adopt(capturedPath);
    _pages.add(page);
    notifyListeners();
    _prepareInBackground(page.id);
  }

  /// Replaces the photo of page [index] in place: same position, crop
  /// cleared, filter and rotation kept.
  Future<void> retake(int index, String capturedPath) async {
    final old = _pages[index];
    final fresh = await _adopt(capturedPath);
    _pages[index] = old.copyWith(originalPath: fresh.originalPath, clearDerived: true);
    _deleteFile(old.originalPath);
    _deleteDerived(old);
    notifyListeners();
    _preparing.remove(old.id);
    _prepareInBackground(old.id);
  }

  int _indexOf(String id) => _pages.indexWhere((p) => p.id == id);

  ScanPage? pageById(String id) {
    final i = _indexOf(id);
    return i < 0 ? null : _pages[i];
  }

  final Map<String, Future<void>> _preparing = {};

  /// Completes once page [id] has its working copy and detected corners,
  /// starting that work (off the UI thread, in the Rust pool) if needed.
  /// Throws if the photo cannot be processed.
  Future<void> whenPrepared(String id) {
    final page = pageById(id);
    if (page == null || page.isPrepared) return Future.value();
    return _preparing[id] ??= _startPrepare(page);
  }

  Future<void> _startPrepare(ScanPage page) {
    late final Future<void> f;
    f = _prepare(page).whenComplete(() {
      // A retake may have replaced this entry with a newer run.
      if (identical(_preparing[page.id], f)) _preparing.remove(page.id);
    });
    return f;
  }

  void _prepareInBackground(String id) {
    // Failures surface when the user opens the page (whenPrepared retries).
    whenPrepared(id).catchError((Object _) {});
  }

  Future<void> _prepare(ScanPage page) async {
    final working = '${_dir.path}/${page.id}_w.jpg';
    try {
      final size = await bridge.imageSizeFile(path: page.originalPath);
      final full = [
        bridge.Corner(x: 0, y: 0),
        bridge.Corner(x: size.width.toDouble(), y: 0),
        bridge.Corner(x: size.width.toDouble(), y: size.height.toDouble()),
        bridge.Corner(x: 0, y: size.height.toDouble()),
      ];
      // Full-frame "crop" with the Original filter: just a downscaled,
      // EXIF-oriented copy. Corners are bounds-inclusive pixel edges.
      final ws = await bridge.processPageFile(
        srcPath: page.originalPath,
        corners: full,
        filter: bridge.ScanFilter.original,
        rotationQuarters: 0,
        outPath: working,
        maxLongSide: _workingLongSide,
        jpegQuality: 90,
      );
      // Detect on the small copy (fast) and scale up to the original.
      final det = await bridge.detectPageFile(srcPath: working, rotationQuarters: 0);
      final sx = size.width / ws.width, sy = size.height / ws.height;
      final fullRect = fullFrame(Size(size.width.toDouble(), size.height.toDouble()));
      final detected = det.found
          ? [for (final c in det.corners) Offset(c.x * sx, c.y * sy)]
          : fullRect;

      final i = _indexOf(page.id);
      if (i < 0 || _pages[i].originalPath != page.originalPath) {
        // Deleted or retaken meanwhile. A deleted page that is restored by
        // Undo simply gets prepared again when it is next opened.
        _deleteFile(working);
        return;
      }
      _pages[i] = _pages[i].copyWith(
        imageSize: Size(size.width.toDouble(), size.height.toDouble()),
        workingPath: working,
        workingSize: Size(ws.width.toDouble(), ws.height.toDouble()),
        corners: detected,
        detectedCorners: detected,
      );
      notifyListeners();
      await _refreshThumb(page.id);
    } catch (_) {
      _deleteFile(working);
      rethrow;
    }
  }

  /// The whole image as a corner list.
  static List<Offset> fullFrame(Size s) =>
      [Offset.zero, Offset(s.width, 0), Offset(s.width, s.height), Offset(0, s.height)];

  /// Stores new crop settings for page [id] and re-renders its thumbnail.
  Future<void> setEdits(String id,
      {required List<Offset> corners, required PageFilter filter, required int rotationQuarterTurns}) {
    final i = _indexOf(id);
    if (i < 0) return Future.value();
    _pages[i] = _pages[i].copyWith(
        corners: corners, filter: filter, rotationQuarterTurns: rotationQuarterTurns);
    notifyListeners();
    return _refreshThumb(id);
  }

  final Map<String, int> _thumbRev = {};

  Future<void> _refreshThumb(String id) async {
    final page = pageById(id);
    final corners = page?.effectiveWorkingCorners;
    if (page == null || corners == null) return;
    final rev = _thumbRev[id] = (_thumbRev[id] ?? 0) + 1;
    // A fresh name per render so Image.file never shows a cached old one.
    final out = '${_dir.path}/${id}_t$rev.jpg';
    try {
      await bridge.processPageFile(
        srcPath: page.workingPath!,
        corners: toBridgeCorners(corners),
        filter: toBridgeFilter(page.filter),
        rotationQuarters: 0,
        outPath: out,
        maxLongSide: _thumbLongSide,
        jpegQuality: 80,
      );
    } catch (_) {
      return; // The list falls back to the original photo.
    }
    final i = _indexOf(id);
    if (i < 0 || _thumbRev[id] != rev) {
      _deleteFile(out); // Superseded or deleted meanwhile.
      return;
    }
    final old = _pages[i].thumbPath;
    _pages[i] = _pages[i].copyWith(thumbPath: out);
    if (old != null) _deleteFile(old);
    notifyListeners();
  }

  /// A path in the session's private directory for temporary renders.
  String scratchPath(String name) => '${_dir.path}/$name';

  void _deleteDerived(ScanPage p) {
    if (p.workingPath != null) _deleteFile(p.workingPath!);
    if (p.thumbPath != null) _deleteFile(p.thumbPath!);
  }

  /// [newIndex] is the final position (ReorderableListView.onReorderItem).
  void reorder(int oldIndex, int newIndex) {
    _pages.insert(newIndex, _pages.removeAt(oldIndex));
    notifyListeners();
  }

  /// Removes a page from the list but keeps its file so [restore] can undo.
  /// Call [discard] once the deletion is final.
  ScanPage remove(int index) {
    final page = _pages.removeAt(index);
    notifyListeners();
    return page;
  }

  void restore(int index, ScanPage page) {
    _pages.insert(index.clamp(0, _pages.length), page);
    notifyListeners();
  }

  void discard(ScanPage page) {
    _deleteFile(page.originalPath);
    _deleteDerived(page);
  }

  void _deleteFile(String path) {
    try {
      File(path).deleteSync();
    } on FileSystemException {
      // Already gone.
    }
  }
}
