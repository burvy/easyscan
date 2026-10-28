import 'dart:io';
import 'dart:ui';

import 'package:path_provider/path_provider.dart';

import '../models/scan_page.dart';
import '../src/rust/api.dart';
import 'scan_session.dart';

/// JPEG quality presets for the exported PDF.
enum ExportQuality {
  small(60),
  standard(85);

  const ExportQuality(this.jpegQuality);
  final int jpegQuality;
}

/// Turns the ordered pages into one PDF in the app cache. Each page is
/// processed from its original with its crop, filter and rotation, then the
/// processed JPEGs are assembled. Runs on the Rust side, so the UI stays
/// responsive while it works.
class ExportService {
  /// Writes `<fileName>.pdf` and returns it. [onProgress] reports
  /// (steps done, total steps): one step per page, plus one for assembly.
  static Future<File> exportPdf({
    required ScanSession session,
    required String fileName,
    required PdfPageSize pageSize,
    required ExportQuality quality,
    void Function(int done, int total)? onProgress,
  }) async {
    final cache = await getTemporaryDirectory();

    // Previous exports were shared already; keep only the latest.
    final exports = Directory('${cache.path}/exports');
    if (exports.existsSync()) exports.deleteSync(recursive: true);
    exports.createSync(recursive: true);

    final work = Directory('${cache.path}/export_work');
    if (work.existsSync()) work.deleteSync(recursive: true);
    work.createSync(recursive: true);

    final pages = List.of(session.pages);
    final total = pages.length + 1;
    final processed = <String>[];
    for (var i = 0; i < pages.length; i++) {
      // A page only has corners once preparation has run; waiting here also
      // starts it for pages the user never opened. Throws if the photo fails.
      await session.whenPrepared(pages[i].id);
      final page = session.pageById(pages[i].id) ?? pages[i];
      final out = '${work.path}/page_${i.toString().padLeft(4, '0')}.jpg';
      await processPageFile(
        srcPath: page.originalPath,
        corners: toBridgeCorners(page.effectiveCorners ?? _fullFrame(page)),
        filter: toBridgeFilter(page.filter),
        rotationQuarters: 0,
        outPath: out,
        maxLongSide: null,
        jpegQuality: quality.jpegQuality,
      );
      processed.add(out);
      onProgress?.call(i + 1, total);
    }

    final outPath = '${exports.path}/${safeFileName(fileName)}.pdf';
    await exportPdfFile(
      pages: processed,
      outPath: outPath,
      pageSize: pageSize,
      jpegQuality: quality.jpegQuality,
    );
    work.deleteSync(recursive: true);
    onProgress?.call(total, total);
    return File(outPath);
  }

  /// Whole oriented image, TL, TR, BR, BL. Only used if a page somehow has no
  /// corners after preparation; the normal path always has them.
  static List<Offset> _fullFrame(ScanPage page) {
    final s = page.imageSize!;
    return [
      const Offset(0, 0),
      Offset(s.width, 0),
      Offset(s.width, s.height),
      Offset(0, s.height),
    ];
  }

  /// Removes characters that are invalid in file names on Android and common
  /// desktops. Falls back to a fixed name if nothing usable is left.
  static String safeFileName(String name) {
    final cleaned = name.replaceAll(RegExp(r'[\\/:*?"<>|\x00-\x1f]'), '').trim();
    return cleaned.isEmpty ? 'Scan' : cleaned;
  }
}
