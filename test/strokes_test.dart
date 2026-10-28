import 'package:easyscan/models/strokes.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  Stroke stroke({
    StrokeTool tool = StrokeTool.pen,
    int color = 0xFF141414,
    double width = 0.008,
    List<Offset> points = const [Offset(0.1, 0.2), Offset(0.3, 0.4)],
  }) =>
      Stroke(tool: tool, color: color, width: width, points: points);

  group('rotateQuarterTurns', () {
    test('one clockwise turn moves the old top-left to the top-right', () {
      final s = stroke(
        points: const [
          Offset(0, 0), // old TL
          Offset(1, 1), // old BR
          Offset(0.5, 0.5),
        ],
      );
      final r = rotateQuarterTurns(s, 1);
      // Verified against rotateCornerOrder: [BL, TL, TR, BR] means the old
      // TL (0,0) reads as the new TR (1,0), and old TL's (0,0) becomes
      // (1 - 0, 0) = (1, 0); old BR (1,1) -> (0, 1).
      expect(r.points[0], const Offset(1, 0));
      expect(r.points[1], const Offset(0, 1));
    });

    test('points are lossless across four turns', () {
      final s = stroke(points: const [Offset(0.13, 0.27), Offset(0.91, 0.05)]);
      var r = s;
      for (var i = 0; i < 4; i++) {
        r = rotateQuarterTurns(r, 1);
      }
      expect(r.points[0].dx, closeTo(0.13, 1e-12));
      expect(r.points[0].dy, closeTo(0.27, 1e-12));
      expect(r.points[1].dx, closeTo(0.91, 1e-12));
      expect(r.points[1].dy, closeTo(0.05, 1e-12));
    });

    test('negative and >4 turns wrap', () {
      final s = stroke(points: const [Offset(0.25, 0.75)]);
      expect(
        rotateQuarterTurns(s, 5).points[0],
        rotateQuarterTurns(s, 1).points[0],
      );
      expect(
        rotateQuarterTurns(s, -3).points[0],
        rotateQuarterTurns(s, 1).points[0],
      );
    });
  });

  group('decimateStrokePoints', () {
    test('a straight tail collapses to its endpoints', () {
      final points = [
        const Offset(0, 0),
        for (var i = 1; i < 20; i++) Offset(i / 40, i / 40), // on the line
        const Offset(0.6, 0.6),
      ];
      final out = decimateStrokePoints(points, 0.002);
      expect(out, hasLength(2));
      expect(out.first, points.first);
      expect(out.last, points.last);
    });

    test('a clear corner is kept, the legs collapse', () {
      final pts = <Offset>[
        for (var i = 0; i <= 10; i++) Offset(i * 0.02, 0),
        for (var i = 1; i <= 10; i++) Offset(0.2, -i * 0.02),
      ];
      final out = decimateStrokePoints(pts, 0.002);
      // Endpoints plus the 90-degree corner.
      expect(out, hasLength(3));
      expect(out[1], const Offset(0.2, 0));
    });

    test('two points come back unchanged', () {
      final two = const [Offset(0, 0), Offset(1, 1)];
      expect(decimateStrokePoints(two, 0.002), two);
    });
  });

  group('mapPointsAcrossRecrop', () {
    // A rectangle crop of a 400x300 photo.
    final base = const [
      Offset(40, 30),
      Offset(360, 30),
      Offset(360, 270),
      Offset(40, 270),
    ];
    const mid = Offset(0.5, 0.5);
    const off = Offset(0.25, 0.75);

    test('same quad is exactly identity', () {
      final out = mapPointsAcrossRecrop(
        points: const [mid, off],
        oldQuad: base,
        newQuad: base,
      );
      expect(out, isNotNull);
      expect(out![0].dx, closeTo(0.5, 1e-9));
      expect(out[0].dy, closeTo(0.5, 1e-9));
      expect(out[1], off);
    });

    test('a shifted crop translates the points with the paper', () {
      // New quad is the same rectangle shifted 20 px down the photo: a
      // mark anchored to the photo content moves 20 px up in frame terms.
      final shifted = base.map((c) => Offset(c.dx, c.dy + 20)).toList();
      final out = mapPointsAcrossRecrop(
        points: const [mid],
        oldQuad: base,
        newQuad: shifted,
      )!;
      // Frame covers photo rows 30..270 before and 50..290 after, so the
      // frame height is still 240 and the old centre row (150 px) sits at
      // normalized (150 - 50) / 240.
      expect(out[0].dx, closeTo(0.5, 1e-9));
      expect(out[0].dy, closeTo((150 - 50) / 240, 1e-9));
    });

    test('a smaller crop zooms the points', () {
      final zoom = const [
        Offset(80, 60),
        Offset(320, 60),
        Offset(320, 240),
        Offset(80, 240),
      ];
      final out = mapPointsAcrossRecrop(
        points: const [off],
        oldQuad: base,
        newQuad: zoom,
      )!;
      // Old frame 320x240 at photo (40,30); off = (80, 180) within it ->
      // photo (120, 210) -> new frame at (80,60) size 240x180 -> (40, 150)
      // -> normalized (1/6, 5/6).
      expect(out[0].dx, closeTo(1 / 6, 1e-9));
      expect(out[0].dy, closeTo(5 / 6, 1e-9));
    });

    test('a perspective quad round-trips old -> new -> old', () {
      final warped = const [
        Offset(60, 20),
        Offset(380, 45),
        Offset(350, 280),
        Offset(30, 255),
      ];
      final forward = mapPointsAcrossRecrop(
        points: const [mid, off, Offset(0.9, 0.1)],
        oldQuad: base,
        newQuad: warped,
      )!;
      final back = mapPointsAcrossRecrop(
        points: forward,
        oldQuad: warped,
        newQuad: base,
      )!;
      expect(back[0].dx, closeTo(0.5, 1e-6));
      expect(back[0].dy, closeTo(0.5, 1e-6));
      expect(back[1].dx, closeTo(0.25, 1e-6));
      expect(back[1].dy, closeTo(0.75, 1e-6));
      expect(back[2].dx, closeTo(0.9, 1e-6));
      expect(back[2].dy, closeTo(0.1, 1e-6));
    });

    test('a degenerate quad returns null', () {
      expect(
        mapPointsAcrossRecrop(points: const [mid], oldQuad: base, newQuad: [
          const Offset(100, 100),
          const Offset(120, 100),
          const Offset(110, 100),
          const Offset(100, 100),
        ]),
        isNull,
      );
    });
  });

  group('quadFrameSize', () {
    test('matches the warp: average edge length, rounded', () {
      final size = quadFrameSize(const [
        Offset(0, 0),
        Offset(120.4, 0),
        Offset(120.4, 60.6),
        Offset(0, 60.6),
      ]);
      expect(size.width, 120);
      expect(size.height, 61);
    });
  });

  group('Stroke JSON', () {
    test('round-trips', () {
      final s = stroke(
        tool: StrokeTool.whiteout,
        color: 0xFFFAF5EB,
        width: 0.016,
        points: const [Offset(0.0, 0.0), Offset(0.5, 0.25), Offset(1, 1)],
      );
      final back = Stroke.fromJson(s.toJson())!;
      expect(back.tool, StrokeTool.whiteout);
      expect(back.color, s.color);
      expect(back.width, 0.016);
      expect(back.points, hasLength(3));
      expect(back.points[1].dx, closeTo(0.5, 1e-9));
      expect(back.points[2], const Offset(1, 1));
    });

    test('rounds points to 4 decimals', () {
      final s = stroke(points: const [Offset(0.12345678, 0.2), Offset(0.3, 0.4)]);
      expect(s.toJson()['pts'], [0.1235, 0.2, 0.3, 0.4]);
    });

    test('malformed records parse to null', () {
      expect(Stroke.fromJson(null), isNull);
      expect(Stroke.fromJson('pen'), isNull);
      expect(Stroke.fromJson({}), isNull);
      expect(Stroke.fromJson({'tool': 'marker', 'color': 1, 'w': 1, 'pts': [0, 0, 1, 1]}), isNull);
      expect(Stroke.fromJson({'tool': 'pen', 'w': 1, 'pts': [0, 0, 1, 1]}), isNull);
      expect(Stroke.fromJson({'tool': 'pen', 'color': 5, 'pts': [0, 0, 1, 1]}), isNull);
      expect(Stroke.fromJson({'tool': 'pen', 'color': 5, 'w': 0.1, 'pts': []}), isNull);
      expect(Stroke.fromJson({'tool': 'pen', 'color': 5, 'w': 0.1, 'pts': [0, 0]}), isNull,
          reason: 'one point draws nothing');
      expect(Stroke.fromJson({'tool': 'pen', 'color': 5, 'w': 0.1, 'pts': [0, 0, 1]}), isNull,
          reason: 'odd length is not pairs');
      expect(
        Stroke.fromJson({'tool': 'pen', 'color': 5, 'w': 0.1, 'pts': [0, double.infinity, 1, 1]}),
        isNull,
      );
      expect(Stroke.fromJson({'tool': 'pen', 'color': 5, 'w': 'fat', 'pts': [0, 0, 1, 1]}), isNull);
    });

    test('out-of-frame coordinates survive the round-trip', () {
      final s = stroke(points: const [Offset(-0.2, 1.9), Offset(0.3, 0.4)]);
      expect(Stroke.fromJson(s.toJson())!.points[0], const Offset(-0.2, 1.9));
    });
  });
}
