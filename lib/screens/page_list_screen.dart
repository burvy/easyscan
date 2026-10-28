import 'dart:io';

import 'package:flutter/material.dart';

import '../models/scan_page.dart';
import '../state/scan_session.dart';
import 'camera_screen.dart';

/// Home screen: the pages of the current scan, drag to reorder, tap for
/// actions. Tasks 010 (crop/filter) and 011 (export) hook in at the marked
/// placeholders.
class PageListScreen extends StatelessWidget {
  const PageListScreen({super.key, required this.session});

  final ScanSession session;

  void _openCamera(BuildContext context, {int? retakeIndex}) {
    Navigator.of(context).push(MaterialPageRoute<void>(
      builder: (_) => CameraScreen(session: session, retakeIndex: retakeIndex),
    ));
  }

  void _delete(BuildContext context, int index) {
    final page = session.remove(index);
    final messenger = ScaffoldMessenger.of(context);
    messenger.clearSnackBars();
    messenger
        .showSnackBar(SnackBar(
          content: const Text('Page deleted'),
          action: SnackBarAction(label: 'Undo', onPressed: () => session.restore(index, page)),
        ))
        .closed
        .then((reason) {
      if (reason != SnackBarClosedReason.action) session.discard(page);
    });
  }

  void _showActions(BuildContext context, int index) {
    showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      builder: (sheet) => SafeArea(
        child: Column(mainAxisSize: MainAxisSize.min, children: [
          ListTile(
            leading: const Icon(Icons.camera_alt_outlined),
            title: const Text('Retake'),
            onTap: () {
              Navigator.pop(sheet);
              _openCamera(context, retakeIndex: index);
            },
          ),
          // Placeholder for task 010 (crop and filter).
          const ListTile(
            enabled: false,
            leading: Icon(Icons.crop),
            title: Text('Crop & filter'),
            subtitle: Text('Coming soon'),
          ),
          ListTile(
            leading: const Icon(Icons.delete_outline),
            title: const Text('Delete'),
            onTap: () {
              Navigator.pop(sheet);
              _delete(context, index);
            },
          ),
        ]),
      ),
    );
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
        listenable: session,
        builder: (context, _) {
          final pages = session.pages;
          return Scaffold(
            appBar: AppBar(
              title: const Text('EasyScan'),
              actions: [
                // Placeholder for task 011 (export and share).
                TextButton(
                  onPressed: pages.isEmpty
                      ? null
                      : () => ScaffoldMessenger.of(context)
                          .showSnackBar(const SnackBar(content: Text('Export is coming soon'))),
                  child: const Text('Export'),
                ),
              ],
            ),
            body: pages.isEmpty
                ? const _EmptyState()
                : ReorderableListView.builder(
                    padding: const EdgeInsets.only(bottom: 96),
                    itemCount: pages.length,
                    onReorderItem: session.reorder,
                    buildDefaultDragHandles: false,
                    itemBuilder: (context, i) => _PageTile(
                      key: ValueKey(pages[i].id),
                      page: pages[i],
                      index: i,
                      onTap: () => _showActions(context, i),
                    ),
                  ),
            floatingActionButton: FloatingActionButton.extended(
              onPressed: () => _openCamera(context),
              icon: const Icon(Icons.camera_alt),
              label: Text(pages.isEmpty ? 'Scan' : 'Add pages'),
            ),
          );
        },
      );
}

class _PageTile extends StatelessWidget {
  const _PageTile({super.key, required this.page, required this.index, required this.onTap});

  final ScanPage page;
  final int index;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) => ListTile(
        onTap: onTap,
        leading: ClipRRect(
          borderRadius: BorderRadius.circular(4),
          child: SizedBox(
            width: 56,
            height: 72,
            // Decode small: full 12 MP bitmaps in a list would exhaust memory.
            child: Image.file(File(page.originalPath),
                cacheWidth: 168, fit: BoxFit.cover, gaplessPlayback: true),
          ),
        ),
        title: Text('Page ${index + 1}'),
        // Dragging starts straight from the handle; it needs no long press.
        trailing: ReorderableDragStartListener(
          index: index,
          child: const Padding(padding: EdgeInsets.all(12), child: Icon(Icons.drag_handle)),
        ),
      );
}

class _EmptyState extends StatelessWidget {
  const _EmptyState();

  @override
  Widget build(BuildContext context) => const Center(
        child: Padding(
          padding: EdgeInsets.all(32),
          child: Column(mainAxisSize: MainAxisSize.min, children: [
            Icon(Icons.document_scanner_outlined, size: 72),
            SizedBox(height: 16),
            Text('Scan your homework', style: TextStyle(fontSize: 20)),
            SizedBox(height: 8),
            Text('Photograph each page, then reorder, retake or delete them before exporting a PDF.',
                textAlign: TextAlign.center),
          ]),
        ),
      );
}
