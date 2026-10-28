import 'dart:ui';

/// Filter applied when a page is rendered. Mirrors the core's filters but is
/// owned by Dart so the page model does not depend on generated bridge code.
enum PageFilter { original, grayscale, enhance, blackWhite }

/// One scanned page. The original photo is never modified: crop, filter and
/// rotation are re-applied from it on demand, so edits are non-destructive and
/// a retake just swaps the original.
class ScanPage {
  ScanPage({
    required this.id,
    required this.originalPath,
    this.corners,
    // Enhance whitens paper and keeps coloured ink, the best default for
    // homework photos; "Original" would leave shadows and a grey cast.
    this.filter = PageFilter.enhance,
    this.rotationQuarterTurns = 0,
  });

  /// Unique and never reused, so it is a stable key and image-cache key.
  final String id;

  /// JPEG in the app's private storage.
  final String originalPath;

  /// TL, TR, BR, BL in original-image pixels; null until detection has run
  /// (or after a retake). Null means "use the full image".
  final List<Offset>? corners;

  final PageFilter filter;

  /// Clockwise quarter turns applied after crop, 0-3.
  final int rotationQuarterTurns;

  ScanPage copyWith({
    String? originalPath,
    List<Offset>? corners,
    bool clearCorners = false,
    PageFilter? filter,
    int? rotationQuarterTurns,
  }) =>
      ScanPage(
        id: id,
        originalPath: originalPath ?? this.originalPath,
        corners: clearCorners ? null : (corners ?? this.corners),
        filter: filter ?? this.filter,
        rotationQuarterTurns: rotationQuarterTurns ?? this.rotationQuarterTurns,
      );
}
