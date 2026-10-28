import 'dart:typed_data';
import 'dart:ui';

/// Samples the paper colour next to a white-out stroke, so the frozen colour
/// blends with the page it covers: hard white glares on warm or shadowed
/// paper, and under the B&W filter a pale sample binarizes to paper white
/// either way.
///
/// [centerline] is the stroke polyline in working-copy pixels, [thicknessPx]
/// its thickness in the same frame and [rgba] the decoded working copy
/// ([width] x [height]). Pixels in a band 1.5-2.5 thicknesses either side of
/// the path are collected; ink-like ones are dropped against the band's own
/// bright percentile, and the median of the rest is the answer. With too few
/// survivors (the stroke ran through dense writing) the page's own bright
/// pixels answer instead, and failing that plain white.
Color samplePaperColor({
  required List<Offset> centerline,
  required double thicknessPx,
  required Uint8List rgba,
  required int width,
  required int height,
}) {
  const minSamples = 8;
  const white = Color(0xFFFFFFFF);
  if (width <= 0 || height <= 0 || rgba.length < width * height * 4) return white;

  Color? medianOfBright(List<int> offsets, double quantile) {
    if (offsets.length < minSamples) return null;
    double luma(int o) => 0.2126 * rgba[o] + 0.7152 * rgba[o + 1] + 0.0722 * rgba[o + 2];
    final lumas = [for (final o in offsets) luma(o)]..sort();
    // Relative paper trick: anything well below the bright end is ink.
    final threshold = lumas[((lumas.length - 1) * quantile).round()] * 0.85;
    final kept = [
      for (final o in offsets)
        if (luma(o) >= threshold) o,
    ];
    if (kept.length < minSamples) return null;
    int median(int channel) {
      final v = [for (final o in kept) rgba[o + channel]]..sort();
      return v[v.length ~/ 2];
    }

    return Color.fromARGB(255, median(0), median(1), median(2));
  }

  // The band around the path, clear of the ink by construction.
  final band = <int>[];
  if (thicknessPx > 0) {
    final step = thicknessPx / 2 < 2 ? 2.0 : thicknessPx / 2;
    for (var i = 0; i + 1 < centerline.length; i++) {
      final a = centerline[i], seg = centerline[i + 1] - a;
      final len = seg.distance;
      if (len <= 0) continue;
      final normal = Offset(-seg.dy, seg.dx) / len;
      final steps = (len / step).ceil();
      for (var s = 0; s <= steps; s++) {
        final p = a + seg * (s / steps);
        for (final m in const [1.5, 2.0, 2.5]) {
          for (final side in const [1.0, -1.0]) {
            final q = p + normal * (m * thicknessPx * side);
            final x = q.dx.round(), y = q.dy.round();
            if (x >= 0 && x < width && y >= 0 && y < height) band.add((y * width + x) * 4);
          }
        }
      }
    }
  }
  final local = medianOfBright(band, 0.90);
  if (local != null) return local;

  // Page-wide fallback: a strided subsample, keeping its brightest pixels.
  final total = width * height;
  final stride = total ~/ 4096 < 1 ? 1 : total ~/ 4096;
  return medianOfBright([for (var i = 0; i < total; i += stride) i * 4], 0.97) ?? white;
}
