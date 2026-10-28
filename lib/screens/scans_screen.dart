import 'dart:io';

import 'package:flutter/material.dart';

import '../state/scan_session.dart';

/// Past scans: tap one to reopen it for editing and export.
class ScansScreen extends StatefulWidget {
  const ScansScreen({super.key, required this.session});

  final ScanSession session;

  @override
  State<ScansScreen> createState() => _ScansScreenState();
}

class _ScansScreenState extends State<ScansScreen> {
  late Future<List<ScanSummary>> _scans = widget.session.listScans();

  void _reload() {
    final scans = widget.session.listScans();
    setState(() => _scans = scans);
  }

  Future<void> _open(ScanSummary scan) async {
    final nav = Navigator.of(context);
    await widget.session.openScan(scan.id);
    nav.pop();
  }

  Future<void> _rename(ScanSummary scan) async {
    final controller = TextEditingController(text: scan.name);
    final name = await showDialog<String>(
      context: context,
      builder: (dialog) => AlertDialog(
        title: const Text('Rename scan'),
        content: TextField(controller: controller, autofocus: true, textCapitalization: TextCapitalization.sentences),
        actions: [
          TextButton(onPressed: () => Navigator.pop(dialog), child: const Text('Cancel')),
          TextButton(onPressed: () => Navigator.pop(dialog, controller.text), child: const Text('Rename')),
        ],
      ),
    );
    if (name == null) return;
    if (scan.isCurrent) {
      widget.session.rename(name);
    } else {
      await widget.session.renameScan(scan.id, name);
    }
    _reload();
  }

  Future<void> _delete(ScanSummary scan) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (dialog) => AlertDialog(
        title: Text('Delete "${scan.name}"?'),
        content: Text('Its ${scan.pageCount} ${scan.pageCount == 1 ? 'page' : 'pages'} will be deleted.'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(dialog, false), child: const Text('Cancel')),
          TextButton(onPressed: () => Navigator.pop(dialog, true), child: const Text('Delete')),
        ],
      ),
    );
    if (ok != true) return;
    await widget.session.deleteScan(scan.id);
    _reload();
  }

  @override
  Widget build(BuildContext context) => Scaffold(
        appBar: AppBar(title: const Text('Past scans')),
        body: FutureBuilder<List<ScanSummary>>(
          future: _scans,
          builder: (context, snap) {
            final scans = snap.data;
            if (scans == null) return const Center(child: CircularProgressIndicator());
            if (scans.isEmpty) {
              return const Center(child: Padding(padding: EdgeInsets.all(32), child: Text('No scans yet.')));
            }
            final total = scans.fold<int>(0, (sum, s) => sum + s.bytes);
            return Column(children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 8, 16, 4),
                child: Align(
                  alignment: Alignment.centerLeft,
                  child: Text('${scans.length} ${scans.length == 1 ? 'scan' : 'scans'} · ${_size(total)} on this phone',
                      style: Theme.of(context).textTheme.bodySmall),
                ),
              ),
              Expanded(
                child: ListView.builder(
                  itemCount: scans.length,
                  itemBuilder: (context, i) => _ScanTile(
                    scan: scans[i],
                    onTap: () => _open(scans[i]),
                    onRename: () => _rename(scans[i]),
                    onDelete: () => _delete(scans[i]),
                  ),
                ),
              ),
            ]);
          },
        ),
      );
}

String _size(int bytes) {
  if (bytes < 1024 * 1024) return '${(bytes / 1024).ceil()} KB';
  return '${(bytes / (1024 * 1024)).toStringAsFixed(1)} MB';
}

String _date(DateTime t) {
  const months = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
  String two(int n) => n.toString().padLeft(2, '0');
  return '${t.day} ${months[t.month - 1]} ${two(t.hour)}:${two(t.minute)}';
}

class _ScanTile extends StatelessWidget {
  const _ScanTile({required this.scan, required this.onTap, required this.onRename, required this.onDelete});

  final ScanSummary scan;
  final VoidCallback onTap;
  final VoidCallback onRename;
  final VoidCallback onDelete;

  @override
  Widget build(BuildContext context) => ListTile(
        onTap: onTap,
        leading: SizedBox(
          width: 48,
          height: 64,
          child: ClipRRect(
            borderRadius: BorderRadius.circular(4),
            child: scan.thumbPath == null
                ? const ColoredBox(color: Colors.black12)
                : Image.file(File(scan.thumbPath!), fit: BoxFit.cover, cacheWidth: 144,
                    errorBuilder: (_, _, _) => const ColoredBox(color: Colors.black12)),
          ),
        ),
        title: Text(scan.name, maxLines: 1, overflow: TextOverflow.ellipsis),
        subtitle: Text('${_date(scan.updated)} · ${scan.pageCount} ${scan.pageCount == 1 ? 'page' : 'pages'} · ${_size(scan.bytes)}'
            '${scan.isCurrent ? ' · open now' : ''}'),
        trailing: PopupMenuButton<String>(
          onSelected: (v) => v == 'rename' ? onRename() : onDelete(),
          itemBuilder: (_) => const [
            PopupMenuItem(value: 'rename', child: Text('Rename')),
            PopupMenuItem(value: 'delete', child: Text('Delete')),
          ],
        ),
      );
}
