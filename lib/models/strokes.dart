import 'dart:math' as math;
import 'dart:ui';

/// Tool a stroke was drawn with. A `pen` carries its ink colour; a
/// `whiteout` carries the paper colour sampled where it was drawn.
enum StrokeTool { pen, whiteout }

/// One annotation stroke, stored non-destructively.
///
/// All strokes live in the page's **result frame** (post-crop, post-rotate):
/// `points` are normalized to [0..1]² of that frame and `width` is the line
/// thickness as a fraction of its short side. The same stroke data then
/// renders in the editor (Dart), at thumbnails and burned into full-res
/// exports (Rust) with no further scaling math.
class Stroke {
  const Stroke({
    required this.tool,
    required this.color,
    required this.width,
    required this.points,
  });

  final StrokeTool tool;

  /// Opaque ARGB. For a pen this is an offered ink colour (the curated
  /// palette survives the B&W rule-line eraser); for a white-out it is the
  /// paper colour frozen at commit time.
  final int color;

  /// Line thickness as a fraction of `min(result width, result height)`.
  final double width;

  /// Polyline, ≥ 2 points, normalized [0..1]² of the result frame. Points
  /// slightly outside the frame are allowed and clipped at render time.
  final List<Offset> points;

  Map<String, Object> toJson() {
    return {
      'tool': switch (tool) {
        StrokeTool.pen => 'pen',
        StrokeTool.whiteout => 'whiteout',
      },
      'color': color,
      'w': width,
      'pts': [
        for (final p in points) ...[
          _round4(p.dx),
          _round4(p.dy),
        ]
      ],
    };
  }

  /// Lenient parse: anything malformed (wrong types, unknown tool, fewer
  /// than two points) returns null, so one bad record cannot break a
  /// session's loading.
  static Stroke? fromJson(Object? json) {
    if (json is! Map) return null;
    final tool = switch (json['tool']) {
      'pen' => StrokeTool.pen,
      'whiteout' => StrokeTool.whiteout,
      _ => null,
    };
    final w = json['w'] is num ? (json['w'] as num).toDouble() : null;
    final color = json['color'] is num ? (json['color'] as num).toInt() : null;
    final pts = _parsePoints(json['pts']);
    if (tool == null || w == null || color == null || pts == null) return null;
    return Stroke(tool: tool, color: color, width: w, points: pts);
  }

  static List<Offset>? _parsePoints(Object? raw) {
    if (raw is! List) return null;
    final values = raw.map((v) => v is num ? v.toDouble() : null).toList();
    if (values.any((v) => v == null || v.isNaN || v.isInfinite)) return null;
    if (values.length.isOdd || values.length < 4) return null;
    final points = <Offset>[];
    for (var i = 0; i + 1 < values.length; i += 2) {
      points.add(Offset(values[i]!, values[i + 1]!));
    }
    return points;
  }

  @override
  String toString() =>
      'Stroke($tool 0x${color.toRadixString(16)} w=$width pts=${points.length})';
}

double _round4(double v) => (v * 10000).round() / 10000;

/// One clockwise quarter-turn of the result frame moves a normalized point
/// `(nx, ny)` to `(1 - ny, nx)` — the old top-left lands on the new
/// top-right, matching `rotateCornerOrder` in scan_page.dart. Lossless, so
/// strokes follow a rotation instead of being cleared. Widths are a
/// fraction of the short side, which a quarter-turn preserves.
Stroke rotateQuarterTurns(Stroke stroke, int quarterTurns) {
  final k = quarterTurns % 4;
  if (k == 0) return stroke;
  Offset map(Offset p) => switch (k) {
        1 => Offset(1 - p.dy, p.dx),
        2 => Offset(1 - p.dx, 1 - p.dy),
        _ => Offset(p.dy, 1 - p.dx), // 3
      };
  return Stroke(
    tool: stroke.tool,
    color: stroke.color,
    width: stroke.width,
    points: stroke.points.map(map).toList(),
  );
}

/// Ramer–Douglas–Peucker decimation for committed strokes: drops points
/// that bend within [tolerance] (normalized units; the suggested call-site
/// value is 0.002, ≈ 4 px at the working copy's scale). Endpoints are
/// always kept.
List<Offset> decimateStrokePoints(List<Offset> points, double tolerance) {
  if (points.length <= 2) return List.of(points);
  final keep = List<bool>.filled(points.length, false);
  keep[0] = keep[points.length - 1] = true;
  final ranges = <(int, int)>[(0, points.length - 1)];
  while (ranges.isNotEmpty) {
    final (first, last) = ranges.removeLast();
    var farthest = -1;
    var farDistance = tolerance;
    for (var i = first + 1; i < last; i++) {
      final d = _distanceToLine(points[i], points[first], points[last]);
      if (d > farDistance) {
        farthest = i;
        farDistance = d;
      }
    }
    if (farthest < 0) continue;
    keep[farthest] = true;
    ranges.addAll([(first, farthest), (farthest, last)]);
  }
  return [for (var i = 0; i < points.length; i++) if (keep[i]) points[i]];
}

/// Perpendicular distance from [p] to the straight line through [a] and
/// [b] (falling back to the point distance when a and b coincide).
double _distanceToLine(Offset p, Offset a, Offset b) {
  final ab = b - a;
  final ap = p - a;
  if (ab.distance <= 0) return ap.distance;
  return (ab.dy * ap.dx - ab.dx * ap.dy).abs() / ab.distance;
}

/// Sizes the core's warp output for a quad: the average of the two edge
/// lengths per dimension, rounded, at least 1 — the same formula as
/// `detect::warp` in `core/src/detect.rs`, so normalized stroke
/// coordinates here and the pixels they burn into there agree.
Size quadFrameSize(List<Offset> quad) {
  double d(Offset q, Offset r) =>
      math.sqrt((q.dx - r.dx) * (q.dx - r.dx) + (q.dy - r.dy) * (q.dy - r.dy));
  final w = ((d(quad[0], quad[1]) + d(quad[3], quad[2])) / 2).round();
  final h = ((d(quad[0], quad[3]) + d(quad[1], quad[2])) / 2).round();
  return Size(w.clamp(1, double.maxFinite).toDouble(),
      h.clamp(1, double.maxFinite).toDouble());
}

/// Maps stroke points across a recrop, anchored to the photo content: old
/// result -> photo pixels -> new result, both hops via the same 3x3
/// perspective homographies the core's warp uses (forward plus inverse),
/// so this is exact rather than approximate. Returns null for a degenerate
/// quad — the caller clears the strokes (with the recrop warning).
///
/// Points may end up outside the new frame; the renderer clips strokes
/// that stray and the caller may drop fully-out-of-frame ones.
List<Offset>? mapPointsAcrossRecrop({
  required List<Offset> points,
  required List<Offset> oldQuad,
  required List<Offset> newQuad,
}) {
  if (points.isEmpty) return points;
  final hOld = _homography(_homeFrame(oldQuad), oldQuad);
  final hNew = _homography(_homeFrame(newQuad), newQuad);
  if (hOld == null || hNew == null) return null;
  final hNewInv = _invert(hNew);
  if (hNewInv == null) return null;
  final oldFrame = quadFrameSize(oldQuad);
  final frame = quadFrameSize(newQuad);

  Offset remap(Offset p) {
    // Result frame -> photo. The half-pixel bookkeeping mirrors the warp
    // (pixel centres sit +0.5 into a pixel), which is what makes a
    // same-quad remap exactly identity. First the normalized point goes
    // to old-frame pixel positions, the frame this homography was solved
    // for.
    final inOld = Offset(p.dx * oldFrame.width, p.dy * oldFrame.height);
    final photo =
        _applyH(hOld, inOld + const Offset(0.5, 0.5)) - const Offset(0.5, 0.5);
    // Photo -> new result frame, before dividing out to normalized.
    final next = _applyH(hNewInv, photo + const Offset(0.5, 0.5)) -
        const Offset(0.5, 0.5);
    return Offset(next.dx / frame.width, next.dy / frame.height);
  }

  return [for (final p in points) remap(p)];
}

/// The result-frame corners a quad warps to: (0, 0), (w, 0), (w, h), (0, h)
/// with the warp's own frame size.
List<Offset> _homeFrame(List<Offset> quad) {
  final size = quadFrameSize(quad);
  return [
    Offset.zero,
    Offset(size.width, 0),
    Offset(size.width, size.height),
    Offset(0, size.height),
  ];
}

/// Solves the 3x3 homography (h22 = 1) mapping each `from[i]` to `to[i]`,
/// ported from `detect::homography` so both sides agree to the digit.
/// Returns null when the points do not determine a homography.
List<double>? _homography(List<Offset> from, List<Offset> to) {
  final m = List.generate(8, (_) => List<double>.filled(9, 0));
  for (var i = 0; i < 4; i++) {
    final (x, y) = (from[i].dx, from[i].dy);
    final (u, v) = (to[i].dx, to[i].dy);
    m[2 * i] = [x, y, 1, 0, 0, 0, -u * x, -u * y, u];
    m[2 * i + 1] = [0, 0, 0, x, y, 1, -v * x, -v * y, v];
  }
  for (var c = 0; c < 8; c++) {
    var p = c;
    for (var r = c + 1; r < 8; r++) {
      if (m[r][c].abs() > m[p][c].abs()) p = r;
    }
    if (m[p][c].abs() < 1e-12) return null;
    if (p != c) {
      final t = m[c];
      m[c] = m[p];
      m[p] = t;
    }
    for (var r = 0; r < 8; r++) {
      if (r == c) continue;
      final f = m[r][c] / m[c][c];
      for (var k = c; k < 9; k++) {
        m[r][k] -= f * m[c][k];
      }
    }
  }
  return [for (var i = 0; i < 8; i++) m[i][8] / m[i][i]];
}

/// Applies a homography (in the flattened 8-coefficient form, h22 = 1).
Offset _applyH(List<double> h, Offset p) {
  final d = h[6] * p.dx + h[7] * p.dy + 1;
  return Offset(
    (h[0] * p.dx + h[1] * p.dy + h[2]) / d,
    (h[3] * p.dx + h[4] * p.dy + h[5]) / d,
  );
}

/// Inverts a homography, renormalizing so the result has h22 = 1 again.
/// Returns null for the singular cases (e.g. a collinear quad).
List<double>? _invert(List<double> m) {
  final (a, b, c, d, e, f, g, h) =
      (m[0], m[1], m[2], m[3], m[4], m[5], m[6], m[7]);
  // M's determinant and adjugate cofactors; adj22 happens to be ae - bd.
  final det = a * (e - f * h) - b * (d - f * g) + c * (d * h - e * g);
  if (det.abs() < 1e-12) return null;
  final adj22 = a * e - b * d;
  if (adj22.abs() < 1e-12) return null;
  // inv = adj(M) / det, divided once more by (inv[2][2] = adj22 / det) to
  // put the inverse back into the flattened h22 = 1 form.
  return [
    (e - f * h) / adj22,
    (c * h - b) / adj22,
    (b * f - c * e) / adj22,
    (f * g - d) / adj22,
    (a - c * g) / adj22,
    (c * d - a * f) / adj22,
    (d * h - e * g) / adj22,
    (b * g - a * h) / adj22,
  ];
}
