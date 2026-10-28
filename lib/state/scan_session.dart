import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';

import '../models/scan_page.dart';

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
    _pages.add(await _adopt(capturedPath));
    notifyListeners();
  }

  /// Replaces the photo of page [index] in place: same position, corners
  /// cleared, filter and rotation kept.
  Future<void> retake(int index, String capturedPath) async {
    final old = _pages[index];
    final fresh = await _adopt(capturedPath);
    _pages[index] = old.copyWith(originalPath: fresh.originalPath, clearCorners: true);
    _deleteFile(old.originalPath);
    notifyListeners();
  }

  void update(int index, ScanPage page) {
    _pages[index] = page;
    notifyListeners();
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

  void discard(ScanPage page) => _deleteFile(page.originalPath);

  void _deleteFile(String path) {
    try {
      File(path).deleteSync();
    } on FileSystemException {
      // Already gone.
    }
  }
}
