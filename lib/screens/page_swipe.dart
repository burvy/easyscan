import 'package:flutter/material.dart';

/// Reports a horizontal one-finger swipe on [child] as [onSwipe] with +1 (next
/// page, swipe left) or -1 (previous page, swipe right).
///
/// It only watches raw pointer events and never joins the gesture arena, so it
/// cannot steal drags from pinch-zoom or crop handles. A gesture counts only if
/// it stayed a single finger, travelled mostly sideways, and [canSwipe] allowed
/// it where the finger went down (e.g. not zoomed, not on a handle).
class PageSwipe extends StatefulWidget {
  const PageSwipe({
    super.key,
    required this.child,
    required this.onSwipe,
    this.canSwipe,
  });

  final Widget child;
  final ValueChanged<int> onSwipe;

  /// Asked when a finger goes down, with its position in [child]'s coordinates.
  final bool Function(Offset localPosition)? canSwipe;

  @override
  State<PageSwipe> createState() => _PageSwipeState();
}

class _PageSwipeState extends State<PageSwipe> {
  static const _minDistance = 80.0;

  final Set<int> _pointers = {};
  Offset _start = Offset.zero;
  bool _tracking = false;

  void _down(PointerDownEvent e) {
    if (_pointers.isEmpty) {
      _start = e.localPosition;
      _tracking = widget.canSwipe?.call(e.localPosition) ?? true;
    } else {
      _tracking = false; // A second finger: pinch, not a swipe.
    }
    _pointers.add(e.pointer);
  }

  void _up(PointerUpEvent e) {
    if (!_pointers.remove(e.pointer) || _pointers.isNotEmpty) return;
    if (!_tracking) return;
    _tracking = false;
    final d = e.localPosition - _start;
    if (d.dx.abs() >= _minDistance && d.dx.abs() > 2 * d.dy.abs()) {
      widget.onSwipe(d.dx < 0 ? 1 : -1);
    }
  }

  void _cancel(PointerCancelEvent e) {
    _pointers.remove(e.pointer);
    _tracking = false;
  }

  @override
  Widget build(BuildContext context) => Listener(
    behavior: HitTestBehavior.translucent,
    onPointerDown: _down,
    onPointerUp: _up,
    onPointerCancel: _cancel,
    child: widget.child,
  );
}
