import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:ui';

import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';

import '../models/scan_page.dart';
import '../models/strokes.dart';
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

bridge.StrokeSpec toBridgeStrokeSpec(Stroke s) => bridge.StrokeSpec(
      // The bridge's tool ints mirror this enum's order (0 pen, 1 white-out).
      tool: s.tool.index,
      color: s.color,
      width: s.width,
      points: Float64List.fromList(
          [for (final p in s.points) ...[p.dx, p.dy]]),
    );

List<bridge.StrokeSpec> toBridgeStrokeSpecs(List<Stroke> strokes) =>
    [for (final s in strokes) toBridgeStrokeSpec(s)];

/// Lists a stored scan in the history screen.
class ScanSummary {
  ScanSummary({
    required this.id,
    required this.name,
    required this.created,
    required this.updated,
    required this.pageCount,
    required this.thumbPath,
    required this.bytes,
    required this.isCurrent,
  });

  final String id;
  final String name;
  final DateTime created;
  final DateTime updated;
  final int pageCount;

  /// First page's thumbnail, or its original photo if there is none.
  final String? thumbPath;
  final int bytes;
  final bool isCurrent;
}

/// "Scan 27 Oct 14:32": the name a scan has until the user renames it.
String defaultScanName(DateTime t) {
  const months = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
  String two(int n) => n.toString().padLeft(2, '0');
  return 'Scan ${t.day} ${months[t.month - 1]} ${two(t.hour)}:${two(t.minute)}';
}

String _newId() => DateTime.now().microsecondsSinceEpoch.toString();

String _baseName(String path) => path.split(RegExp(r'[\\/]')).last;

/// The scans on disk: `scans/<id>/` folders, each with its own `state.json`
/// and photos, and `scans/current` naming the open one. One scan is open at a
/// time (the "current scan"): its pages are in memory, saved next to its
/// photos after every change and restored on launch, so it survives the app
/// being killed. Other scans stay on disk and are opened with [openScan].
///
/// A scan's folder is created when its first photo arrives, so a scan nobody
/// added to never appears in history.
class ScanSession extends ChangeNotifier {
  ScanSession._(this._scansDir);

  static const _stateFile = 'state.json';
  static const _currentFile = 'current';

  final Directory _scansDir;
  final List<ScanPage> _pages = [];
  int _counter = 0;

  String _id = _newId();
  String _name = ''; // empty: show the default name
  int _created = DateTime.now().millisecondsSinceEpoch;

  /// Bumped whenever another scan is loaded, so that work started for the
  /// previous one (preparation, thumbnails) can tell it is stale.
  int _gen = 0;

  String get _path => '${_scansDir.path}/$_id';

  String get currentId => _id;
  String get name => _name.isEmpty ? defaultScanName(DateTime.fromMillisecondsSinceEpoch(_created)) : _name;

  /// Filter given to newly captured pages: the one chosen most recently. Kept
  /// with the scan (and across New scan, which is the same user habit).
  PageFilter get defaultFilter => _defaultFilter;
  PageFilter _defaultFilter = PageFilter.enhance;

  List<ScanPage> get pages => List.unmodifiable(_pages);
  int get length => _pages.length;

  /// Opens the scan that was open last, if any. Pages whose photo is missing
  /// or whose entry is corrupt are skipped; files not belonging to a page are
  /// deleted. [root] is the app's documents folder.
  static Future<ScanSession> create([Directory? root]) async {
    root ??= await getApplicationDocumentsDirectory();
    migrateLegacy(root);
    final scans = Directory('${root.path}/scans')..createSync(recursive: true);
    final session = ScanSession._(scans);
    try {
      final id = File('${scans.path}/$_currentFile').readAsStringSync().trim();
      if (id.isNotEmpty && File('${scans.path}/$id/$_stateFile').existsSync()) {
        session._id = id;
        session._load();
      }
    } on FileSystemException {
      // No current scan yet.
    }
    if (session._pages.isEmpty) {
      // Nothing worth keeping (e.g. every page was deleted before a kill).
      session._deleteFolder(session._path);
      session._startFresh();
    }
    return session;
  }

  /// Before history there was a single scan in `pages/`. Moves it into
  /// `scans/` as the current scan (a rename, so nothing is copied).
  @visibleForTesting
  static void migrateLegacy(Directory root) {
    final old = Directory('${root.path}/pages');
    final scans = Directory('${root.path}/scans');
    if (!old.existsSync() || scans.existsSync()) return;
    try {
      if (!File('${old.path}/$_stateFile').existsSync()) {
        old.deleteSync(recursive: true);
        return;
      }
      final id = _newId();
      scans.createSync(recursive: true);
      old.renameSync('${scans.path}/$id');
      File('${scans.path}/$_currentFile').writeAsStringSync(id);
    } on FileSystemException {
      // Leave it; the next launch tries again.
    }
  }

  void _load() {
    try {
      final file = File('$_path/$_stateFile');
      if (file.existsSync()) {
        final json = jsonDecode(file.readAsStringSync());
        final counter = json['counter'];
        if (counter is int) _counter = counter;
        final filter = json['defaultFilter'];
        if (filter is String) {
          _defaultFilter = PageFilter.values.asNameMap()[filter] ?? _defaultFilter;
        }
        final name = json['name'];
        if (name is String) _name = name;
        final created = json['created'];
        _created = created is int ? created : file.lastModifiedSync().millisecondsSinceEpoch;
        for (final j in json['pages'] as List) {
          final page = ScanPageJson.fromJson(j, _path, (p) => File(p).existsSync());
          if (page != null) _pages.add(page);
        }
      }
    } catch (_) {
      // Unreadable state: start empty; orphan cleanup below removes the files.
    }
    _deleteOrphans();
    for (final p in _pages) {
      if (!p.isPrepared) _prepareInBackground(p.id);
    }
  }

  void _deleteOrphans() {
    final keep = <String>{_stateFile};
    for (final p in _pages) {
      for (final path in [p.originalPath, p.workingPath, p.thumbPath]) {
        if (path != null) keep.add(_baseName(path));
      }
    }
    try {
      for (final e in Directory(_path).listSync()) {
        if (e is File && !keep.contains(_baseName(e.path))) _deleteFile(e.path);
      }
    } on FileSystemException {
      // Best effort.
    }
  }

  /// Forgets the open scan's in-memory state and makes an empty, unsaved one
  /// current. The old scan's files are the caller's business.
  void _startFresh() {
    _gen++;
    _pages.clear();
    _preparing.clear();
    _thumbRev.clear();
    _id = _newId();
    _name = '';
    _created = DateTime.now().millisecondsSinceEpoch;
    _writeCurrent();
  }

  void _writeCurrent() {
    try {
      File('${_scansDir.path}/$_currentFile').writeAsStringSync(_id);
    } on FileSystemException {
      // Only matters for the next launch.
    }
  }

  /// Saves, then stops using the open scan: an empty one is deleted, a stored
  /// one gives up its working copies (rebuilt when it is opened again).
  Future<void> _leave() async {
    await _flush();
    if (_pages.isEmpty) {
      _deleteFolder(_path);
    } else {
      for (final p in _pages) {
        if (p.workingPath != null) _deleteFile(p.workingPath!);
      }
    }
  }

  /// Starts an empty scan; the one that was open stays in history.
  Future<void> newScan() async {
    if (_pages.isEmpty) return;
    await _leave();
    _startFresh();
    notifyListeners();
  }

  Future<void> openScan(String id) async {
    if (id == _id) return;
    if (!File('${_scansDir.path}/$id/$_stateFile').existsSync()) return;
    await _leave();
    _startFresh();
    _id = id;
    _load();
    _writeCurrent();
    notifyListeners();
  }

  /// Deletes scan [id] and its files. Deleting the open one leaves an empty
  /// new scan open.
  Future<void> deleteScan(String id) async {
    if (id == _id) return clear();
    _deleteFolder('${_scansDir.path}/$id');
  }

  /// Deletes the open scan with all its files and starts an empty one.
  Future<void> clear() async {
    final old = _path;
    _startFresh();
    notifyListeners();
    await _saveFuture; // Let an already running write finish first.
    _deleteFolder(old);
  }

  void rename(String name) {
    _name = name.trim();
    notifyListeners();
  }

  /// Renames a stored scan; the open one is renamed with [rename].
  Future<void> renameScan(String id, String name) async {
    if (id == _id) return rename(name);
    try {
      final file = File('${_scansDir.path}/$id/$_stateFile');
      final json = jsonDecode(await file.readAsString()) as Map;
      json['name'] = name.trim();
      final tmp = File('${file.path}.tmp');
      await tmp.writeAsString(jsonEncode(json), flush: true);
      await tmp.rename(file.path);
    } on Exception {
      // Unreadable scan: leave it alone.
    }
  }

  /// Every stored scan with at least one page, newest first. The open scan is
  /// included (saved first so its numbers are current).
  Future<List<ScanSummary>> listScans() async {
    await _flush();
    final out = <ScanSummary>[];
    for (final e in _scansDir.listSync()) {
      if (e is! Directory) continue;
      final id = _baseName(e.path);
      try {
        final file = File('${e.path}/$_stateFile');
        final json = jsonDecode(file.readAsStringSync()) as Map;
        final pages = json['pages'] as List;
        if (pages.isEmpty) continue;
        final first = pages.first as Map;
        var thumb = first['thumb'] as String?;
        if (thumb == null || !File('${e.path}/$thumb').existsSync()) thumb = first['original'] as String?;
        var bytes = 0;
        for (final f in e.listSync()) {
          if (f is File) bytes += f.lengthSync();
        }
        final modified = file.lastModifiedSync();
        final created = json['created'];
        final createdAt = created is int ? DateTime.fromMillisecondsSinceEpoch(created) : modified;
        final name = json['name'];
        out.add(ScanSummary(
          id: id,
          name: name is String && name.isNotEmpty ? name : defaultScanName(createdAt),
          created: createdAt,
          updated: modified,
          pageCount: pages.length,
          thumbPath: thumb == null ? null : '${e.path}/$thumb',
          bytes: bytes,
          isCurrent: id == _id,
        ));
      } catch (_) {
        // Not a readable scan: leave it out.
      }
    }
    out.sort((a, b) => b.updated.compareTo(a.updated));
    return out;
  }

  // Saving is coalesced: any number of changes in one turn cause one write, and
  // writes never overlap (they are chained). The file is replaced atomically
  // via a temp file.
  bool _saveQueued = false;
  Future<void> _saveFuture = Future.value();

  @override
  void notifyListeners() {
    super.notifyListeners();
    _enqueueSave();
  }

  void _enqueueSave() {
    if (_saveQueued) return;
    _saveQueued = true;
    _saveFuture = _saveFuture.then((_) {
      _saveQueued = false;
      return _save();
    });
  }

  /// Completes once everything changed so far is on disk.
  Future<void> _flush() {
    _enqueueSave();
    return _saveFuture;
  }

  Future<void> _save() async {
    // Everything that depends on the open scan is captured before the first
    // await, so a switch while writing cannot mix two scans.
    final dir = Directory(_path);
    if (_pages.isEmpty && !dir.existsSync()) return;
    final text = jsonEncode({
      'version': 2,
      'name': _name,
      'created': _created,
      'counter': _counter,
      'defaultFilter': _defaultFilter.name,
      'pages': [for (final p in _pages) p.toJson()],
    });
    try {
      await dir.create(recursive: true);
      final tmp = File('${dir.path}/$_stateFile.tmp');
      await tmp.writeAsString(text, flush: true);
      await tmp.rename('${dir.path}/$_stateFile');
    } on FileSystemException {
      // Not fatal: the next change tries again.
    }
  }

  /// Moves a freshly captured photo into private storage under a new,
  /// never-reused name (so a retake cannot show a stale cached thumbnail).
  Future<ScanPage> _adopt(String capturedPath) async {
    final id = '${DateTime.now().microsecondsSinceEpoch}_${_counter++}';
    final dest = '$_path/$id.jpg';
    await Directory(_path).create(recursive: true);
    await File(capturedPath).copy(dest);
    try {
      await File(capturedPath).delete();
    } on FileSystemException {
      // The camera's cache file is disposable anyway.
    }
    return ScanPage(id: id, originalPath: dest);
  }

  Future<void> addPhoto(String capturedPath) async {
    final page = (await _adopt(capturedPath)).copyWith(filter: _defaultFilter);
    _pages.add(page);
    notifyListeners();
    _prepareInBackground(page.id);
  }

  /// Replaces the photo of page [index] in place: same position, crop
  /// cleared, filter and rotation kept.
  Future<void> retake(int index, String capturedPath) async {
    final old = _pages[index];
    final fresh = await _adopt(capturedPath);
    // A retake replaces the photo: annotations on it no longer make sense.
    _pages[index] =
        old.copyWith(originalPath: fresh.originalPath, clearDerived: true, strokes: const []);
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
    final dir = _path, gen = _gen;
    final working = '$dir/${page.id}_w.jpg';
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
      if (gen != _gen) {
        _deleteFile(working); // Another scan was opened meanwhile.
        return;
      }
      if (page.corners != null && page.imageSize != null) {
        // Only the working copy was missing (stored scan): keep the crop.
        final i = _indexOf(page.id);
        if (i < 0 || _pages[i].originalPath != page.originalPath) {
          _deleteFile(working);
          return;
        }
        _pages[i] = _pages[i].copyWith(
          workingPath: working,
          workingSize: Size(ws.width.toDouble(), ws.height.toDouble()),
        );
        notifyListeners();
        if (_pages[i].thumbPath == null) await _refreshThumb(page.id);
        return;
      }
      // Detect on the small copy (fast) and scale up to the original.
      final det = await bridge.detectPageFile(srcPath: working, rotationQuarters: 0);
      final sx = size.width / ws.width, sy = size.height / ws.height;
      final fullRect = fullFrame(Size(size.width.toDouble(), size.height.toDouble()));
      final detected = det.found
          ? [for (final c in det.corners) Offset(c.x * sx, c.y * sy)]
          : fullRect;

      final i = _indexOf(page.id);
      if (gen != _gen || i < 0 || _pages[i].originalPath != page.originalPath) {
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

  /// Remembers [filter] as the one new pages start with.
  void setDefaultFilter(PageFilter filter) {
    if (filter == _defaultFilter) return;
    _defaultFilter = filter;
    notifyListeners();
  }

  /// Gives every page [filter] (crop and rotation stay per page) and makes it
  /// the default for new pages.
  void applyFilterToAll(PageFilter filter) {
    _defaultFilter = filter;
    for (var i = 0; i < _pages.length; i++) {
      if (_pages[i].filter == filter) continue;
      _pages[i] = _pages[i].copyWith(filter: filter);
      _refreshThumb(_pages[i].id);
    }
    notifyListeners();
  }

  /// Replaces page [id]'s annotation strokes and re-renders the thumbnail so
  /// the list shows the ink (strokes are burned in at render time; no image
  /// file ever holds them durably).
  Future<void> setStrokes(String id, List<Stroke> strokes) {
    final i = _indexOf(id);
    if (i < 0) return Future.value();
    _pages[i] = _pages[i].copyWith(strokes: List.unmodifiable(strokes));
    notifyListeners();
    return _refreshThumb(id);
  }

  /// Stores new crop settings for page [id] and re-renders its thumbnail.
  Future<void> setEdits(String id,
      {required List<Offset> corners, required PageFilter filter, required int rotationQuarterTurns}) {
    final i = _indexOf(id);
    if (i < 0) return Future.value();
    final before = _pages[i];
    final edited = before.copyWith(
        corners: corners, filter: filter, rotationQuarterTurns: rotationQuarterTurns);
    // Strokes are anchored to the paper: they follow a rotation or recrop.
    _pages[i] = edited.copyWith(strokes: strokesAfterEdit(before, edited));
    notifyListeners();
    return _refreshThumb(id);
  }

  final Map<String, int> _thumbRev = {};

  Future<void> _refreshThumb(String id) async {
    final page = pageById(id);
    final corners = page?.effectiveWorkingCorners;
    if (page == null || corners == null) return;
    final gen = _gen;
    final rev = _thumbRev[id] = (_thumbRev[id] ?? 0) + 1;
    // A fresh name per render so Image.file never shows a cached old one.
    final out = '$_path/${id}_t$rev.jpg';
    try {
      await bridge.processPageFile(
        srcPath: page.workingPath!,
        corners: toBridgeCorners(corners),
        filter: toBridgeFilter(page.filter),
        rotationQuarters: 0,
        strokes: page.strokes.isEmpty ? null : toBridgeStrokeSpecs(page.strokes),
        outPath: out,
        maxLongSide: _thumbLongSide,
        jpegQuality: 80,
      );
    } catch (_) {
      return; // The list falls back to the original photo.
    }
    final i = _indexOf(id);
    if (gen != _gen || i < 0 || _thumbRev[id] != rev) {
      _deleteFile(out); // Superseded or deleted meanwhile.
      return;
    }
    final old = _pages[i].thumbPath;
    _pages[i] = _pages[i].copyWith(thumbPath: out);
    if (old != null) _deleteFile(old);
    notifyListeners();
  }

  /// A path in the session's private directory for temporary renders.
  String scratchPath(String name) => '$_path/$name';

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

  void _deleteFolder(String path) {
    try {
      final d = Directory(path);
      if (d.existsSync()) d.deleteSync(recursive: true);
    } on FileSystemException {
      // Best effort.
    }
  }

  void _deleteFile(String path) {
    try {
      File(path).deleteSync();
    } on FileSystemException {
      // Already gone.
    }
  }
}

/// Strokes of [before] carried over to [after]'s crop and rotation, anchored
/// to the paper: mapped photo-exact through both quads, widths rescaled to
/// the new frame's short side, strokes left wholly outside the new frame
/// dropped. A degenerate quad clears them.
List<Stroke> strokesAfterEdit(ScanPage before, ScanPage after) {
  if (before.strokes.isEmpty) return before.strokes;
  final oldQuad = before.effectiveCorners, newQuad = after.effectiveCorners;
  if (oldQuad == null || newQuad == null) return before.strokes;
  bool same = true;
  for (var i = 0; i < 4; i++) {
    if ((oldQuad[i] - newQuad[i]).distanceSquared > 1e-9) same = false;
  }
  if (same) return before.strokes;
  double shortSide(List<Offset> q) {
    final s = quadFrameSize(q);
    return s.width < s.height ? s.width : s.height;
  }

  final widthScale = shortSide(oldQuad) / shortSide(newQuad);
  final out = <Stroke>[];
  for (final s in before.strokes) {
    final pts = mapPointsAcrossRecrop(points: s.points, oldQuad: oldQuad, newQuad: newQuad);
    if (pts == null) return const [];
    if (pts.every((p) => p.dx < 0 || p.dx > 1 || p.dy < 0 || p.dy > 1)) continue;
    out.add(Stroke(tool: s.tool, color: s.color, width: s.width * widthScale, points: pts));
  }
  return out;
}
