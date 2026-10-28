import 'dart:io';

import 'package:flutter/material.dart';
import 'package:share_plus/share_plus.dart';

import '../src/rust/api.dart';
import '../state/export_service.dart';
import '../state/scan_session.dart';

/// Export options for the next PDF. Kept for this app session only; they go
/// back to the defaults on the next launch.
class _ExportChoices {
  static PdfPageSize pageSize = PdfPageSize.a4;
  static ExportQuality quality = ExportQuality.standard;
}

/// Asks for a file name, page size and quality, exports the pages in order,
/// then opens the share sheet with the PDF. Once the share sheet closes, offers
/// to start a new scan.
Future<void> showExportDialog(BuildContext context, ScanSession session) async {
  final shared = await showDialog<bool>(
    context: context,
    barrierDismissible: false,
    builder: (_) => _ExportDialog(session: session),
  );
  if (shared == true && context.mounted) await _offerNewScan(context, session);
}

/// The PDF is already out; clearing is the user's choice, not a side effect.
Future<void> _offerNewScan(BuildContext context, ScanSession session) async {
  final start = await showDialog<bool>(
    context: context,
    builder: (dialog) => AlertDialog(
      title: const Text('Start a new scan?'),
      content: const Text('Your PDF is exported. Clear these pages to start a new scan?'),
      actions: [
        TextButton(onPressed: () => Navigator.pop(dialog, false), child: const Text('Keep pages')),
        FilledButton(
          onPressed: () => Navigator.pop(dialog, true),
          child: const Text('Start a new scan'),
        ),
      ],
    ),
  );
  if (start == true) session.clear();
}

class _ExportDialog extends StatefulWidget {
  const _ExportDialog({required this.session});

  final ScanSession session;

  @override
  State<_ExportDialog> createState() => _ExportDialogState();
}

class _ExportDialogState extends State<_ExportDialog> {
  late final TextEditingController _name =
      TextEditingController(text: _defaultName(DateTime.now()));
  PdfPageSize _pageSize = _ExportChoices.pageSize;
  ExportQuality _quality = _ExportChoices.quality;

  // Progress of a running export: null while idle.
  int? _done;
  int? _total;
  // True while the share sheet is open; the dialog stays until it closes.
  bool _sharing = false;

  bool get _busy => _done != null;

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  /// "Scan 2026-10-28 14.05", the default name in the file field.
  static String _defaultName(DateTime t) {
    String two(int v) => v.toString().padLeft(2, '0');
    return 'Scan ${t.year}-${two(t.month)}-${two(t.day)} ${two(t.hour)}.${two(t.minute)}';
  }

  Future<void> _export() async {
    _ExportChoices.pageSize = _pageSize;
    _ExportChoices.quality = _quality;
    setState(() {
      _done = 0;
      _total = widget.session.length + 1;
    });

    try {
      final file = await ExportService.exportPdf(
        session: widget.session,
        fileName: _name.text.trim(),
        pageSize: _pageSize,
        quality: _quality,
        onProgress: (done, total) {
          if (mounted) setState(() => _done = done);
        },
      );
      if (!mounted) return;
      setState(() => _sharing = true);
      await _share(file);
      if (mounted) Navigator.pop(context, true);
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _done = null;
        _total = null;
        _sharing = false;
      });
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Export failed: $e')),
      );
    }
  }

  Future<void> _share(File file) async {
    final name = file.uri.pathSegments.last.replaceAll('.pdf', '');
    await SharePlus.instance.share(ShareParams(
      files: [XFile(file.path, mimeType: 'application/pdf', name: '$name.pdf')],
      subject: name,
    ));
  }

  @override
  Widget build(BuildContext context) {
    final progress = _total == null ? null : _done! / _total!;
    return AlertDialog(
      title: const Text('Export PDF'),
      content: SingleChildScrollView(
        child: Column(mainAxisSize: MainAxisSize.min, children: [
          if (_sharing) ...[
            const LinearProgressIndicator(),
            const SizedBox(height: 8),
            const Text('Sharing the PDF...'),
          ] else if (_busy) ...[
            LinearProgressIndicator(value: progress),
            const SizedBox(height: 8),
            Text('Preparing page ${(_done! + 1).clamp(1, widget.session.length)} '
                'of ${widget.session.length}'),
          ] else ...[
            TextField(
              controller: _name,
              decoration: const InputDecoration(labelText: 'File name', suffixText: '.pdf'),
              onChanged: (_) => setState(() {}),
            ),
            const SizedBox(height: 16),
            DropdownButtonFormField<PdfPageSize>(
              initialValue: _pageSize,
              decoration: const InputDecoration(labelText: 'Page size'),
              items: const [
                DropdownMenuItem(value: PdfPageSize.fitToImage, child: Text('Fit to photo')),
                DropdownMenuItem(value: PdfPageSize.a4, child: Text('A4')),
                DropdownMenuItem(value: PdfPageSize.letter, child: Text('US Letter')),
              ],
              onChanged: (v) => setState(() => _pageSize = v!),
            ),
            const SizedBox(height: 16),
            SegmentedButton<ExportQuality>(
              segments: const [
                ButtonSegment(value: ExportQuality.small, label: Text('Smaller file')),
                ButtonSegment(value: ExportQuality.standard, label: Text('Standard')),
              ],
              selected: {_quality},
              onSelectionChanged: (s) => setState(() => _quality = s.first),
            ),
          ],
        ]),
      ),
      actions: [
        TextButton(
          onPressed: _busy ? null : () => Navigator.pop(context),
          child: const Text('Cancel'),
        ),
        FilledButton(
          onPressed: _busy || _name.text.trim().isEmpty ? null : _export,
          child: const Text('Export'),
        ),
      ],
    );
  }
}
