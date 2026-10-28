import 'dart:ui';

/// Filter applied when a page is rendered. Mirrors the core's filters but is
/// owned by Dart so the page model does not depend on generated bridge code.
enum PageFilter { original, grayscale, enhance, blackWhite }

/// Rotates a TL, TR, BR, BL corner list by [quarterTurns] clockwise turns.
///
/// The warp maps the first corner to the output's top-left, so turning the
/// page is just cycling the list: one turn gives [BL, TL, TR, BR]. The
/// corners stay in one frame and the image itself is never re-rotated.
List<T> rotateCornerOrder<T>(List<T> corners, int quarterTurns) {
  assert(corners.length == 4);
  final k = quarterTurns % 4;
  return [for (var i = 0; i < 4; i++) corners[(i - k) % 4]];
}

/// True if the quad (any consistent corner order) is convex, i.e. the handles
/// have not been dragged across each other.
bool isConvexQuad(List<Offset> q) {
  assert(q.length == 4);
  var sign = 0;
  for (var i = 0; i < 4; i++) {
    final a = q[i], b = q[(i + 1) % 4], c = q[(i + 2) % 4];
    final cross = (b.dx - a.dx) * (c.dy - b.dy) - (b.dy - a.dy) * (c.dx - b.dx);
    if (cross == 0) return false;
    final s = cross > 0 ? 1 : -1;
    if (sign != 0 && s != sign) return false;
    sign = s;
  }
  return true;
}

/// One scanned page. The original photo is never modified: crop, filter and
/// rotation are re-applied from it on demand, so edits are non-destructive and
/// a retake just swaps the original.
class ScanPage {
  ScanPage({
    required this.id,
    required this.originalPath,
    this.imageSize,
    this.workingPath,
    this.workingSize,
    this.corners,
    this.detectedCorners,
    this.thumbPath,
    // Enhance whitens paper and keeps coloured ink, the best default for
    // homework photos; "Original" would leave shadows and a grey cast.
    this.filter = PageFilter.enhance,
    this.rotationQuarterTurns = 0,
  });

  /// Unique and never reused, so it is a stable key and image-cache key.
  final String id;

  /// JPEG in the app's private storage.
  final String originalPath;

  /// Size of the original in the EXIF-oriented frame (what the user sees),
  /// not the raw stored pixels. Null until preparation has run.
  final Size? imageSize;

  /// Downscaled, EXIF-oriented copy used for previews and thumbnails; the
  /// original is only decoded again at export.
  final String? workingPath;
  final Size? workingSize;

  /// TL, TR, BR, BL in original-image pixels in the EXIF-oriented frame (the
  /// frame of [imageSize], what `detect_page_file(path, 0)` returns); null
  /// until preparation has run (or after a retake).
  final List<Offset>? corners;

  /// What detection suggested, for "Reset"; same frame as [corners].
  final List<Offset>? detectedCorners;

  /// Small JPEG of the cropped and filtered page for the list.
  final String? thumbPath;

  final PageFilter filter;

  /// Clockwise quarter turns applied after crop, 0-3. Realised by cycling the
  /// corner order ([effectiveCorners]); the bridge always gets 0.
  final int rotationQuarterTurns;

  bool get isPrepared => workingPath != null && corners != null;

  /// [corners] in warp order, i.e. with the rotation applied.
  List<Offset>? get effectiveCorners =>
      corners == null ? null : rotateCornerOrder(corners!, rotationQuarterTurns);

  /// [effectiveCorners] scaled from the original's frame to the working copy.
  List<Offset>? get effectiveWorkingCorners {
    final c = effectiveCorners;
    if (c == null || imageSize == null || workingSize == null) return null;
    final sx = workingSize!.width / imageSize!.width;
    final sy = workingSize!.height / imageSize!.height;
    return [for (final p in c) Offset(p.dx * sx, p.dy * sy)];
  }

  ScanPage copyWith({
    String? originalPath,
    Size? imageSize,
    String? workingPath,
    Size? workingSize,
    List<Offset>? corners,
    List<Offset>? detectedCorners,
    String? thumbPath,
    bool clearDerived = false,
    PageFilter? filter,
    int? rotationQuarterTurns,
  }) =>
      ScanPage(
        id: id,
        originalPath: originalPath ?? this.originalPath,
        imageSize: clearDerived ? null : (imageSize ?? this.imageSize),
        workingPath: clearDerived ? null : (workingPath ?? this.workingPath),
        workingSize: clearDerived ? null : (workingSize ?? this.workingSize),
        corners: clearDerived ? null : (corners ?? this.corners),
        detectedCorners: clearDerived ? null : (detectedCorners ?? this.detectedCorners),
        thumbPath: clearDerived ? null : (thumbPath ?? this.thumbPath),
        filter: filter ?? this.filter,
        rotationQuarterTurns: rotationQuarterTurns ?? this.rotationQuarterTurns,
      );
}
