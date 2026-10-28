import 'dart:ui';

import 'package:easyscan/models/scan_page.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  const tl = Offset(0, 0), tr = Offset(10, 0), br = Offset(10, 20), bl = Offset(0, 20);
  final quad = [tl, tr, br, bl];

  group('rotateCornerOrder', () {
    test('zero turns is identity', () => expect(rotateCornerOrder(quad, 0), quad));
    test('one clockwise turn puts BL first', () => expect(rotateCornerOrder(quad, 1), [bl, tl, tr, br]));
    test('two turns', () => expect(rotateCornerOrder(quad, 2), [br, bl, tl, tr]));
    test('three turns', () => expect(rotateCornerOrder(quad, 3), [tr, br, bl, tl]));
    test('four turns wraps', () => expect(rotateCornerOrder(quad, 4), quad));
  });

  test('isConvexQuad accepts a page outline in either direction', () {
    expect(isConvexQuad(quad), isTrue);
    expect(isConvexQuad(quad.reversed.toList()), isTrue);
  });

  test('isConvexQuad rejects crossed corners', () {
    expect(isConvexQuad([tl, br, tr, bl]), isFalse);
  });

  test('effectiveWorkingCorners rotates and scales to the working copy', () {
    final page = ScanPage(
      id: 'a',
      originalPath: 'a.jpg',
      imageSize: const Size(100, 200),
      workingSize: const Size(50, 100),
      workingPath: 'w.jpg',
      corners: const [Offset(10, 20), Offset(90, 20), Offset(90, 180), Offset(10, 180)],
      rotationQuarterTurns: 1,
    );
    expect(page.effectiveWorkingCorners,
        const [Offset(5, 90), Offset(5, 10), Offset(45, 10), Offset(45, 90)]);
  });
}
