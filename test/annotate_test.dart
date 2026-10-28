import 'dart:typed_data';

import 'package:easyscan/models/paper_sample.dart';
import 'package:easyscan/models/scan_page.dart';
import 'package:easyscan/models/strokes.dart';
import 'package:easyscan/state/scan_session.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  const quad = [Offset(0, 0), Offset(400, 0), Offset(400, 300), Offset(0, 300)];
  ScanPage page(List<Offset> corners, int rot, List<Stroke> strokes) => ScanPage(
        id: 'p',
        originalPath: 'o.jpg',
        corners: corners,
        rotationQuarterTurns: rot,
        strokes: strokes,
      );
  const stroke = Stroke(
      tool: StrokeTool.pen, color: 0xFF000000, width: 0.01, points: [Offset(0.25, 0.5), Offset(0.5, 0.5)]);

  test('unchanged crop keeps strokes untouched', () {
    expect(strokesAfterEdit(page(quad, 0, [stroke]), page(quad, 0, [stroke])).single, same(stroke));
  });

  test('rotation moves strokes like rotateQuarterTurns', () {
    final out = strokesAfterEdit(page(quad, 0, [stroke]), page(quad, 1, [stroke])).single;
    final want = rotateQuarterTurns(stroke, 1);
    for (var i = 0; i < 2; i++) {
      expect((out.points[i] - want.points[i]).distance, lessThan(0.01));
    }
  });

  test('recrop keeps strokes on the paper and drops those outside', () {
    // Crop to the right half: x 200..400 -> old x=0.25 (100px) leaves, 0.75 stays.
    const right = [Offset(200, 0), Offset(400, 0), Offset(400, 300), Offset(0 + 200, 300)];
    const outside = Stroke(
        tool: StrokeTool.pen, color: 0xFF000000, width: 0.01, points: [Offset(0.1, 0.5), Offset(0.2, 0.5)]);
    const inside = Stroke(
        tool: StrokeTool.pen, color: 0xFF000000, width: 0.01, points: [Offset(0.6, 0.5), Offset(0.8, 0.5)]);
    final out = strokesAfterEdit(page(quad, 0, [outside, inside]), page(right, 0, const []));
    expect(out.length, 1);
    expect(out.single.points[0].dx, closeTo(0.2, 0.01));
    expect(out.single.points[1].dx, closeTo(0.6, 0.01));
  });

  test('paper sample ignores ink and picks the surrounding colour', () {
    const w = 100, h = 100;
    final rgba = Uint8List(w * h * 4);
    for (var i = 0; i < w * h; i++) {
      rgba[i * 4] = 230;
      rgba[i * 4 + 1] = 220;
      rgba[i * 4 + 2] = 200;
      rgba[i * 4 + 3] = 255;
    }
    // A dark ink blob on the path.
    for (var y = 45; y < 55; y++) {
      for (var x = 20; x < 80; x++) {
        rgba[(y * w + x) * 4] = 20;
        rgba[(y * w + x) * 4 + 1] = 20;
        rgba[(y * w + x) * 4 + 2] = 20;
      }
    }
    final c = samplePaperColor(
        centerline: const [Offset(20, 50), Offset(80, 50)], thicknessPx: 8, rgba: rgba, width: w, height: h);
    expect((c.r * 255).round(), 230);
    expect((c.b * 255).round(), 200);
  });
}
