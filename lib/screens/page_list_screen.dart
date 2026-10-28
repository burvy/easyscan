import 'dart:io';

import 'package:flutter/material.dart';

import '../ads/ads_feature.dart';
import '../models/scan_page.dart';
import '../state/scan_session.dart';
import 'camera_screen.dart';
import 'crop_screen.dart';
import 'export_dialog.dart';
import 'page_view_screen.dart';
import 'scans_screen.dart';

/// Home screen: the pages of the current scan, drag to reorder. Tap a page to
/// see its result large; long-press for Retake, Crop & filter and Delete.
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

  Future<void> _rename(BuildContext context) async {
    final controller = TextEditingController(text: session.name);
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
    if (name != null) session.rename(name);
  }

  void _openHistory(BuildContext context) {
    Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => ScansScreen(session: session)));
  }

  void _openPage(BuildContext context, int index) {
    Navigator.of(context).push(MaterialPageRoute<void>(
      builder: (_) => PageViewScreen(session: session, pageId: session.pages[index].id),
    ));
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
          ListTile(
            leading: const Icon(Icons.crop),
            title: const Text('Crop & filter'),
            onTap: () {
              Navigator.pop(sheet);
              Navigator.of(context).push(MaterialPageRoute<void>(
                builder: (_) => CropScreen(session: session, pageId: session.pages[index].id),
              ));
            },
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
              title: GestureDetector(
                onTap: pages.isEmpty ? null : () => _rename(context),
                child: Text(pages.isEmpty ? 'EasyScan' : session.name, overflow: TextOverflow.ellipsis),
              ),
              actions: [
                IconButton(
                  tooltip: 'Past scans',
                  icon: const Icon(Icons.history),
                  onPressed: () => _openHistory(context),
                ),
                TextButton(
                  onPressed: pages.isEmpty ? null : session.newScan,
                  child: const Text('New scan'),
                ),
                TextButton(
                  onPressed: pages.isEmpty ? null : () => showExportDialog(context, session),
                  child: const Text('Export'),
                ),
                const AdPrivacyAction(),
              ],
            ),
            bottomNavigationBar: const AdBanner(),
            body: pages.isEmpty
                ? const _EmptyState()
                : ReorderableListView.builder(
                    padding: const EdgeInsets.only(bottom: 96),
                    itemCount: pages.length,
                    onReorderItem: session.reorder,
                    buildDefaultDragHandles: false,
                    itemBuilder: (context, i) => Dismissible(
                      key: ValueKey(pages[i].id),
                      // Swipe left deletes (with Undo); swipe right retakes. The
                      // row never leaves the list itself: the session change does.
                      confirmDismiss: (direction) async {
                        if (direction == DismissDirection.endToStart) {
                          _delete(context, i);
                        } else {
                          _openCamera(context, retakeIndex: i);
                        }
                        return false;
                      },
                      background: _SwipeBackground(
                          color: Theme.of(context).colorScheme.primaryContainer,
                          icon: Icons.camera_alt_outlined,
                          alignment: Alignment.centerLeft),
                      secondaryBackground: _SwipeBackground(
                          color: Theme.of(context).colorScheme.errorContainer,
                          icon: Icons.delete_outline,
                          alignment: Alignment.centerRight),
                      child: _PageTile(
                        page: pages[i],
                        index: i,
                        onTap: () => _openPage(context, i),
                        onLongPress: () => _showActions(context, i),
                      ),
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
  const _PageTile({
    required this.page,
    required this.index,
    required this.onTap,
    required this.onLongPress,
  });

  final ScanPage page;
  final int index;
  final VoidCallback onTap;
  final VoidCallback onLongPress;

  @override
  Widget build(BuildContext context) => ListTile(
        onTap: onTap,
        onLongPress: onLongPress,
        // Vertical breathing room so neighbouring thumbnails never touch.
        contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 6),
        leading: Container(
          width: 56,
          height: 72,
          clipBehavior: Clip.antiAlias,
          // Paper-like card: white backing and a hairline border keep pages with
          // black binarised margins from running into each other.
          decoration: BoxDecoration(
            color: Colors.white,
            borderRadius: BorderRadius.circular(4),
            border: Border.all(color: Theme.of(context).colorScheme.outlineVariant),
          ),
          // Decode small: full 12 MP bitmaps in a list would exhaust memory.
          child: Image.file(File(page.thumbPath ?? page.originalPath),
              cacheWidth: 168, fit: BoxFit.contain, gaplessPlayback: true),
        ),
        title: Text('Page ${index + 1}'),
        // Dragging starts straight from the handle; it needs no long press.
        trailing: ReorderableDragStartListener(
          index: index,
          child: const Padding(padding: EdgeInsets.all(12), child: Icon(Icons.drag_handle)),
        ),
      );
}

class _SwipeBackground extends StatelessWidget {
  const _SwipeBackground({required this.color, required this.icon, required this.alignment});

  final Color color;
  final IconData icon;
  final Alignment alignment;

  @override
  Widget build(BuildContext context) => Container(
        color: color,
        alignment: alignment,
        padding: const EdgeInsets.symmetric(horizontal: 24),
        child: Icon(icon),
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
