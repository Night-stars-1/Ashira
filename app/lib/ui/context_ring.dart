import 'dart:math' as math;

import 'package:flutter/material.dart';

import 'format.dart';
import 'theme.dart';

/// Context used by the current session, as a ring; the exact numbers are in the tooltip.
class ContextRing extends StatelessWidget {
  const ContextRing({super.key, required this.context, required this.window});
  final int context;
  final int? window;

  @override
  Widget build(BuildContext buildContext) {
    final scheme = Theme.of(buildContext).colorScheme;
    final limit = window;
    final share = limit != null && limit > 0 ? math.min(1.0, context / limit) : 0.0;
    final label = limit != null && limit > 0
        ? '上下文 ${(share * 100).round()}%：${tokens(context)} / ${tokens(limit)} tokens'
        : '上下文 ${tokens(context)} tokens';
    final color = share >= .9 ? scheme.error : share >= .7 ? const Color(0xFFF59E0B) : scheme.mutedForeground;
    return Tooltip(
      message: label,
      child: Semantics(
        label: label,
        child: SizedBox.square(
          dimension: 32,
          child: Center(
            child: TweenAnimationBuilder<double>(
              tween: Tween(end: share),
              duration: const Duration(milliseconds: 300),
              builder: (_, value, _) => CustomPaint(size: const Size.square(18), painter: _Ring(value, color, scheme.mutedForeground.withValues(alpha: .3))),
            ),
          ),
        ),
      ),
    );
  }
}

class _Ring extends CustomPainter {
  _Ring(this.share, this.color, this.track);
  final double share;
  final Color color, track;

  @override
  void paint(Canvas canvas, Size size) {
    const stroke = 2.5;
    final rect = Rect.fromLTWH(stroke / 2, stroke / 2, size.width - stroke, size.height - stroke);
    final paint = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = stroke;
    canvas.drawArc(rect, 0, math.pi * 2, false, paint..color = track);
    if (share > 0) canvas.drawArc(rect, -math.pi / 2, math.pi * 2 * share, false, paint..color = color..strokeCap = StrokeCap.round);
  }

  @override
  bool shouldRepaint(_Ring old) => old.share != share || old.color != color || old.track != track;
}
