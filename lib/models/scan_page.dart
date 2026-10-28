import 'dart:ui';

import 'strokes.dart';

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
    this.strokes = const [],
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

  /// Annotation strokes drawn over the result, in the result's frame
  /// ([0..1]², see [Stroke]). Burned into renders at render time; the photo
  /// itself is never touched. A retake replaces the photo, so it clears
  /// these.
  final List<Stroke> strokes;

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
    List<Stroke>? strokes,
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
        strokes: strokes ?? this.strokes,
      );
}

/// Persistence. File paths are stored relative to the pages folder (the app
/// data path can change across reinstalls and updates).
extension ScanPageJson on ScanPage {
  Map<String, Object?> toJson() {
    String? rel(String? p) => p?.split(RegExp(r'[\\/]')).last;
    List<List<double>>? pts(List<Offset>? l) =>
        l == null ? null : [for (final o in l) [o.dx, o.dy]];
    List<double>? size(Size? s) => s == null ? null : [s.width, s.height];
    return {
      'id': id,
      'original': rel(originalPath),
      'imageSize': size(imageSize),
      'working': rel(workingPath),
      'workingSize': size(workingSize),
      'corners': pts(corners),
      'detected': pts(detectedCorners),
      'thumb': rel(thumbPath),
      'filter': filter.name,
      'rotation': rotationQuarterTurns,
      // Omitted when empty: most pages have no annotations.
      'strokes': strokes.isEmpty ? null : [for (final s in strokes) s.toJson()],
    };
  }

  /// Returns null if [j] is malformed or its original photo is gone. Derived
  /// files that are missing are dropped (they are rebuilt on demand).
  static ScanPage? fromJson(Object? j, String dir, bool Function(String) exists) {
    try {
      final m = j as Map;
      final id = m['id'] as String;
      final original = '$dir/${m['original'] as String}';
      if (!exists(original)) return null;
      Size? size(Object? v) {
        if (v == null) return null;
        final l = v as List;
        return Size((l[0] as num).toDouble(), (l[1] as num).toDouble());
      }

      List<Offset>? pts(Object? v) {
        if (v == null) return null;
        final l = [
          for (final p in v as List)
            Offset(((p as List)[0] as num).toDouble(), (p[1] as num).toDouble())
        ];
        return l.length == 4 ? l : null;
      }

      String? file(Object? v) {
        if (v == null) return null;
        final p = '$dir/${v as String}';
        return exists(p) ? p : null;
      }

      final working = file(m['working']);
      final corners = pts(m['corners']);
      final imageSize = size(m['imageSize']);
      final workingSize = size(m['workingSize']);
      // Crop data (corners, sizes) survives a missing working copy: the copy
      // is dropped for stored scans and rebuilt on open without re-detecting.
      final hasCrop = corners != null && imageSize != null;
      final hasWorking = hasCrop && working != null && workingSize != null;
      return ScanPage(
        id: id,
        originalPath: original,
        imageSize: hasCrop ? imageSize : null,
        workingPath: hasWorking ? working : null,
        workingSize: hasWorking ? workingSize : null,
        corners: hasCrop ? corners : null,
        detectedCorners: hasCrop ? pts(m['detected']) : null,
        thumbPath: hasCrop ? file(m['thumb']) : null,
        filter: PageFilter.values.byName(m['filter'] as String),
        rotationQuarterTurns: (m['rotation'] as int) % 4,
        // Strokes are anchored to the crop's result frame; without crop
        // data they have no frame and would land somewhere else after
        // re-detection, so they only ride along with it.
        strokes: hasCrop ? parseStrokes(m['strokes']) : const [],
      );
    } catch (_) {
      return null;
    }
  }
}
