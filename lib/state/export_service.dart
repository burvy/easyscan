import 'dart:io';

import 'package:path_provider/path_provider.dart';

import '../models/scan_page.dart';
import '../src/rust/api.dart';

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
    required List<ScanPage> pages,
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

    final total = pages.length + 1;
    final processed = <String>[];
    for (var i = 0; i < pages.length; i++) {
      final page = pages[i];
      final out = '${work.path}/page_${i.toString().padLeft(4, '0')}.jpg';
      final size = await imageSizeFile(path: page.originalPath);
      await processPageFile(
        srcPath: page.originalPath,
        corners: rotateCorners(
          page.corners?.map((p) => Corner(x: p.dx, y: p.dy)).toList() ??
              fullFrameCorners(size.width, size.height),
          page.rotationQuarterTurns,
        ),
        filter: _scanFilter(page.filter),
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

  /// Corners of the whole oriented image, TL, TR, BR, BL.
  static List<Corner> fullFrameCorners(int width, int height) => [
        Corner(x: 0, y: 0),
        Corner(x: width.toDouble(), y: 0),
        Corner(x: width.toDouble(), y: height.toDouble()),
        Corner(x: 0, y: height.toDouble()),
      ];

  /// Rotating the page clockwise by [quarterTurns] is cycling the corner list
  /// right by the same amount: TL,TR,BR,BL becomes BL,TL,TR,BR after one turn.
  /// The bridge always gets rotation 0, so the photo itself is never re-rotated.
  static List<Corner> rotateCorners(List<Corner> corners, int quarterTurns) {
    final k = quarterTurns % 4;
    return [for (var i = 0; i < 4; i++) corners[(i - k + 4) % 4]];
  }

  /// Removes characters that are invalid in file names on Android and common
  /// desktops. Falls back to a fixed name if nothing usable is left.
  static String safeFileName(String name) {
    final cleaned = name.replaceAll(RegExp(r'[\\/:*?"<>|\x00-\x1f]'), '').trim();
    return cleaned.isEmpty ? 'Scan' : cleaned;
  }

  static ScanFilter _scanFilter(PageFilter filter) => switch (filter) {
        PageFilter.original => ScanFilter.original,
        PageFilter.grayscale => ScanFilter.grayscale,
        PageFilter.enhance => ScanFilter.enhance,
        PageFilter.blackWhite => ScanFilter.blackWhite,
      };
}
