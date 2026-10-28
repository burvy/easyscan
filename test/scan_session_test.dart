import 'dart:convert';
import 'dart:io';

import 'package:easyscan/state/scan_session.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  late Directory root;

  setUp(() => root = Directory.systemTemp.createTempSync('easyscan_test'));
  tearDown(() => root.deleteSync(recursive: true));

  /// A stored scan in [dir] with prepared pages (so no native code is needed).
  void writeScan(Directory dir, {required int pages, Map<String, Object?> header = const {}}) {
    dir.createSync(recursive: true);
    final list = [];
    for (var i = 0; i < pages; i++) {
      File('${dir.path}/p$i.jpg').writeAsStringSync('photo');
      File('${dir.path}/p${i}_w.jpg').writeAsStringSync('work');
      File('${dir.path}/p${i}_t1.jpg').writeAsStringSync('thumb');
      list.add({
        'id': 'p$i',
        'original': 'p$i.jpg',
        'imageSize': [400, 300],
        'working': 'p${i}_w.jpg',
        'workingSize': [200, 150],
        'corners': [
          [0, 0],
          [400, 0],
          [400, 300],
          [0, 300]
        ],
        'detected': null,
        'thumb': 'p${i}_t1.jpg',
        'filter': 'enhance',
        'rotation': 0,
      });
    }
    File('${dir.path}/state.json')
        .writeAsStringSync(jsonEncode({'version': 1, 'counter': pages, 'pages': list, ...header}));
  }

  test('migrates the single-scan pages folder into scans/ as the current scan', () async {
    writeScan(Directory('${root.path}/pages'), pages: 2);
    final session = await ScanSession.create(root);
    expect(Directory('${root.path}/pages').existsSync(), isFalse);
    expect(session.length, 2);
    expect(session.pages.first.corners, isNotNull);
    expect(session.name, startsWith('Scan '));
    final scans = await session.listScans();
    expect(scans, hasLength(1));
    expect(scans.single.pageCount, 2);
    expect(scans.single.isCurrent, isTrue);
    expect(scans.single.bytes, greaterThan(0));
  });

  test('an empty legacy folder is dropped and a fresh scan starts', () async {
    Directory('${root.path}/pages').createSync();
    File('${root.path}/pages/junk.jpg').writeAsStringSync('x');
    final session = await ScanSession.create(root);
    expect(session.length, 0);
    expect(Directory('${root.path}/pages').existsSync(), isFalse);
    expect(await session.listScans(), isEmpty);
  });

  test('a stored scan with no pages is removed on launch', () async {
    writeScan(Directory('${root.path}/pages'), pages: 0);
    final session = await ScanSession.create(root);
    expect(session.length, 0);
    expect(Directory('${root.path}/scans').listSync().whereType<Directory>(), isEmpty);
  });

  test('new, open and delete keep scans apart', () async {
    writeScan(Directory('${root.path}/pages'), pages: 1);
    final session = await ScanSession.create(root);
    final first = session.currentId;
    session.rename('Maths');
    await session.newScan();
    expect(session.length, 0);
    expect(session.currentId, isNot(first));

    // The first scan is still listed and gave up its working copy.
    final scans = await session.listScans();
    expect(scans.map((s) => s.id), [first]);
    expect(scans.single.name, 'Maths');
    expect(File('${root.path}/scans/$first/p0_w.jpg').existsSync(), isFalse);
    expect(File('${root.path}/scans/$first/p0.jpg').existsSync(), isTrue);

    // A relaunch while the empty scan is open comes back empty, history intact.
    final again = await ScanSession.create(root);
    expect(again.length, 0);
    expect((await again.listScans()).single.id, first);

    await again.openScan(first);
    expect(again.length, 1);
    expect(again.name, 'Maths');
    expect(again.pages.single.corners, isNotNull); // crop kept without working copy
    expect(again.pages.single.isPrepared, isFalse);

    await again.deleteScan(first);
    expect(again.length, 0);
    expect(await again.listScans(), isEmpty);
    expect(Directory('${root.path}/scans/$first').existsSync(), isFalse);
  });

  test('launch sweeps orphan files from stored scans, never from an unreadable one', () async {
    final scans = Directory('${root.path}/scans');
    writeScan(Directory('${scans.path}/open'), pages: 1);
    writeScan(Directory('${scans.path}/old'), pages: 1);
    writeScan(Directory('${scans.path}/broken'), pages: 1);
    File('${scans.path}/current').writeAsStringSync('open');
    File('${scans.path}/old/p0_t2.jpg').writeAsStringSync('orphan');
    File('${scans.path}/old/p0_w_x.tmp').writeAsStringSync('orphan');
    File('${scans.path}/open/p0_t9.jpg').writeAsStringSync('orphan');
    File('${scans.path}/broken/p0_t2.jpg').writeAsStringSync('keep me');
    File('${scans.path}/broken/state.json').writeAsStringSync('{not json');

    await ScanSession.create(root);

    bool has(String s) => File('${scans.path}/$s').existsSync();
    expect(has('old/p0_t2.jpg'), isFalse);
    expect(has('old/p0_w_x.tmp'), isFalse);
    expect(has('open/p0_t9.jpg'), isFalse);
    for (final s in ['old', 'open']) {
      expect(has('$s/p0.jpg'), isTrue);
      expect(has('$s/p0_w.jpg'), isTrue);
      expect(has('$s/p0_t1.jpg'), isTrue);
      expect(has('$s/state.json'), isTrue);
    }
    expect(has('broken/p0_t2.jpg'), isTrue);
  });
}
