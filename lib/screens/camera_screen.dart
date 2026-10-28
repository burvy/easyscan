import 'package:camera/camera.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:permission_handler/permission_handler.dart';

import '../state/scan_session.dart';

/// Full-screen camera. In capture mode it keeps taking pages (appended to the
/// session) until the user taps Done. With [retakeIndex] it takes one photo,
/// replaces that page and closes.
class CameraScreen extends StatefulWidget {
  const CameraScreen({super.key, required this.session, this.retakeIndex});

  final ScanSession session;
  final int? retakeIndex;

  @override
  State<CameraScreen> createState() => _CameraScreenState();
}

class _CameraScreenState extends State<CameraScreen> with WidgetsBindingObserver {
  CameraController? _controller;
  PermissionStatus? _permission;
  String? _error;
  bool _busy = false;
  bool _starting = false;

  bool get _retaking => widget.retakeIndex != null;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _start();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _controller?.dispose();
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) {
      // Also covers coming back from the system settings screen.
      if (_controller == null) _start();
    } else if (state == AppLifecycleState.paused) {
      final c = _controller;
      _controller = null;
      c?.dispose();
      if (mounted) setState(() {});
    }
  }

  Future<void> _start({bool ask = false}) async {
    if (_starting) return;
    _starting = true;
    try {
      var status = await Permission.camera.status;
      if (!status.isGranted && ask && !status.isPermanentlyDenied) {
        status = await Permission.camera.request();
      }
      if (!mounted) return;
      setState(() => _permission = status);
      if (!status.isGranted) return;

      final cameras = await availableCameras();
      final back = cameras.firstWhere(
        (c) => c.lensDirection == CameraLensDirection.back,
        orElse: () => cameras.first,
      );
      final controller = CameraController(
        back,
        ResolutionPreset.max,
        enableAudio: false,
        imageFormatGroup: ImageFormatGroup.jpeg,
      );
      await controller.initialize();
      await controller.lockCaptureOrientation(DeviceOrientation.portraitUp);
      if (!mounted) {
        await controller.dispose();
        return;
      }
      setState(() {
        _controller = controller;
        _error = null;
      });
    } on CameraException catch (e) {
      if (mounted) setState(() => _error = e.description ?? e.code);
    } on StateError {
      if (mounted) setState(() => _error = 'No camera found on this device.');
    } finally {
      _starting = false;
    }
  }

  Future<void> _shoot() async {
    final c = _controller;
    if (c == null || _busy || !c.value.isInitialized) return;
    setState(() => _busy = true);
    try {
      final shot = await c.takePicture();
      if (_retaking) {
        await widget.session.retake(widget.retakeIndex!, shot.path);
        if (mounted) Navigator.of(context).pop();
        return;
      }
      await widget.session.addPhoto(shot.path);
    } on CameraException catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Could not take photo: ${e.description ?? e.code}')),
        );
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final permission = _permission;
    return Scaffold(
      backgroundColor: Colors.black,
      body: SafeArea(
        child: permission == null
            ? const Center(child: CircularProgressIndicator())
            : !permission.isGranted
                ? _PermissionDenied(
                    permanent: permission.isPermanentlyDenied || permission.isRestricted,
                    onRetry: () => _start(ask: true),
                    onClose: () => Navigator.of(context).pop(),
                  )
                : _error != null
                    ? _Message(text: _error!, onClose: () => Navigator.of(context).pop())
                    : _buildCamera(),
      ),
    );
  }

  Widget _buildCamera() {
    final c = _controller;
    if (c == null || !c.value.isInitialized) {
      return const Center(child: CircularProgressIndicator());
    }
    return Column(children: [
      Expanded(child: ClipRect(child: Center(child: CameraPreview(c)))),
      Container(
        color: Colors.black,
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
        child: Row(children: [
          Expanded(
            child: Align(
              alignment: Alignment.centerLeft,
              child: TextButton(
                onPressed: () => Navigator.of(context).pop(),
                child: Text(_retaking ? 'Cancel' : 'Done'),
              ),
            ),
          ),
          _ShutterButton(onPressed: _busy ? null : _shoot, busy: _busy),
          Expanded(
            child: Align(
              alignment: Alignment.centerRight,
              child: _retaking
                  ? Text('Retake page ${widget.retakeIndex! + 1}',
                      style: const TextStyle(color: Colors.white70))
                  : ListenableBuilder(
                      listenable: widget.session,
                      builder: (_, _) => Text(
                        '${widget.session.length} ${widget.session.length == 1 ? 'page' : 'pages'}',
                        style: const TextStyle(color: Colors.white, fontSize: 16),
                      ),
                    ),
            ),
          ),
        ]),
      ),
    ]);
  }
}

class _ShutterButton extends StatelessWidget {
  const _ShutterButton({required this.onPressed, required this.busy});

  final VoidCallback? onPressed;
  final bool busy;

  @override
  Widget build(BuildContext context) => Semantics(
        button: true,
        label: 'Take photo',
        child: GestureDetector(
          onTap: onPressed,
          child: Container(
            width: 76,
            height: 76,
            padding: const EdgeInsets.all(5),
            decoration: BoxDecoration(
              shape: BoxShape.circle,
              border: Border.all(color: Colors.white, width: 4),
            ),
            child: Container(
              decoration: BoxDecoration(
                shape: BoxShape.circle,
                color: busy ? Colors.white38 : Colors.white,
              ),
            ),
          ),
        ),
      );
}

class _PermissionDenied extends StatelessWidget {
  const _PermissionDenied({required this.permanent, required this.onRetry, required this.onClose});

  final bool permanent;
  final VoidCallback onRetry;
  final VoidCallback onClose;

  @override
  Widget build(BuildContext context) => Center(
        child: Padding(
          padding: const EdgeInsets.all(32),
          child: Column(mainAxisSize: MainAxisSize.min, children: [
            const Icon(Icons.no_photography_outlined, size: 64, color: Colors.white70),
            const SizedBox(height: 16),
            const Text('Camera access is needed',
                style: TextStyle(color: Colors.white, fontSize: 20), textAlign: TextAlign.center),
            const SizedBox(height: 8),
            Text(
              permanent
                  ? 'EasyScan scans pages with the camera. Turn on the Camera permission in Settings.'
                  : 'EasyScan scans pages with the camera. Photos stay on your phone.',
              style: const TextStyle(color: Colors.white70),
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: 24),
            if (permanent)
              FilledButton(onPressed: openAppSettings, child: const Text('Open Settings'))
            else
              FilledButton(onPressed: onRetry, child: const Text('Allow camera')),
            TextButton(onPressed: onClose, child: const Text('Not now')),
          ]),
        ),
      );
}

class _Message extends StatelessWidget {
  const _Message({required this.text, required this.onClose});

  final String text;
  final VoidCallback onClose;

  @override
  Widget build(BuildContext context) => Center(
        child: Column(mainAxisSize: MainAxisSize.min, children: [
          Text(text, style: const TextStyle(color: Colors.white), textAlign: TextAlign.center),
          TextButton(onPressed: onClose, child: const Text('Close')),
        ]),
      );
}
