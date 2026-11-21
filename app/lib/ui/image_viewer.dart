import 'package:flutter/material.dart';

/// Opens [image] over the whole screen: pinch or scroll to zoom, drag to pan, double tap
/// to zoom in or back, tap (at fit size), the close button or Back to leave.
Future<void> showImageViewer(BuildContext context, ImageProvider image, {String? label}) =>
    Navigator.of(context).push(PageRouteBuilder<void>(
      opaque: false,
      barrierDismissible: true,
      barrierColor: Colors.black.withValues(alpha: 0.92),
      barrierLabel: label ?? '图片',
      pageBuilder: (context, _, _) => _ImageViewer(image: image, label: label),
      transitionsBuilder: (_, animation, _, child) => FadeTransition(opacity: animation, child: child),
    ));

class _ImageViewer extends StatefulWidget {
  const _ImageViewer({required this.image, this.label});
  final ImageProvider image;
  final String? label;

  @override
  State<_ImageViewer> createState() => _ImageViewerState();
}

class _ImageViewerState extends State<_ImageViewer> {
  final _transform = TransformationController();

  @override
  void dispose() {
    _transform.dispose();
    super.dispose();
  }

  /// Double tap toggles between fitting the screen and 2.5× around the tapped point.
  void _toggleZoom(TapDownDetails details) {
    if (_transform.value.getMaxScaleOnAxis() > 1.01) {
      _transform.value = Matrix4.identity();
      return;
    }
    const scale = 2.5;
    final at = details.localPosition;
    _transform.value = Matrix4.identity()
      ..translateByDouble(-at.dx * (scale - 1), -at.dy * (scale - 1), 0, 1)
      ..scaleByDouble(scale, scale, 1, 1);
  }

  @override
  Widget build(BuildContext context) {
    TapDownDetails? doubleTap;
    return Stack(children: [
      // The viewer covers the screen, so a zoomed picture uses all of it, not a box.
      Positioned.fill(
        child: GestureDetector(
          // Tapping closes only at fit size; zoomed in, a stray tap must not throw the view away.
          onTap: () { if (_transform.value.getMaxScaleOnAxis() <= 1.01) Navigator.of(context).maybePop(); },
          onDoubleTapDown: (details) => doubleTap = details,
          onDoubleTap: () { if (doubleTap != null) _toggleZoom(doubleTap!); },
          child: InteractiveViewer(
            transformationController: _transform,
            minScale: 1,
            maxScale: 8,
            child: SizedBox.expand(
              child: Image(
                image: widget.image,
                fit: BoxFit.contain,
                gaplessPlayback: true,
                semanticLabel: widget.label,
                errorBuilder: (_, _, _) => const Center(child: Text('图片无法加载', style: TextStyle(color: Colors.white70))),
              ),
            ),
          ),
        ),
      ),
      Positioned(
        top: MediaQuery.paddingOf(context).top + 8,
        right: 8,
        child: IconButton(
          tooltip: '关闭',
          icon: const Icon(Icons.close, color: Colors.white),
          style: IconButton.styleFrom(backgroundColor: Colors.black45),
          onPressed: () => Navigator.of(context).maybePop(),
        ),
      ),
    ]);
  }
}
