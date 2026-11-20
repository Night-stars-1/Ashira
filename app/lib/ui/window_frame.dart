import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:window_manager/window_manager.dart';

import 'theme.dart';

/// Windows and Linux draw their own title bar: the OS frame is hidden, the
/// app's header rows double as the drag area, and window buttons sit at the
/// end of the top-right header. macOS keeps its native traffic lights.
bool get customFrame => !kIsWeb && (defaultTargetPlatform == TargetPlatform.windows || defaultTargetPlatform == TargetPlatform.linux);

Future<void> setupWindow() async {
  if (!customFrame) return;
  await windowManager.ensureInitialized();
  await windowManager.setTitle('Ashira');
  await windowManager.setMinimumSize(const Size(560, 420));
  await windowManager.setTitleBarStyle(TitleBarStyle.hidden);
}

/// Drags the window, double click maximizes, right click opens the system menu;
/// buttons inside stay clickable.
class WindowDrag extends StatelessWidget {
  const WindowDrag({super.key, required this.child});
  final Widget child;
  @override
  Widget build(BuildContext context) => customFrame ? DragToMoveArea(child: child) : child;
}

/// Minimize, maximize/restore and close; full height of the row they end.
class WindowButtons extends StatefulWidget {
  const WindowButtons({super.key});
  @override
  State<WindowButtons> createState() => _WindowButtonsState();
}

class _WindowButtonsState extends State<WindowButtons> with WindowListener {
  bool _maximized = false;

  @override
  void initState() {
    super.initState();
    if (!customFrame) return;
    windowManager.addListener(this);
    windowManager.isMaximized().then((value) {
      if (mounted) setState(() => _maximized = value);
    });
  }

  @override
  void dispose() {
    if (customFrame) windowManager.removeListener(this);
    super.dispose();
  }

  @override
  void onWindowMaximize() => setState(() => _maximized = true);

  @override
  void onWindowUnmaximize() => setState(() => _maximized = false);

  @override
  Widget build(BuildContext context) {
    if (!customFrame) return const SizedBox.shrink();
    final scheme = Theme.of(context).colorScheme;
    return Row(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      _WindowButton(tooltip: '最小化', icon: Icons.remove, onPressed: windowManager.minimize),
      _WindowButton(
        tooltip: _maximized ? '还原' : '最大化',
        icon: _maximized ? Icons.filter_none : Icons.crop_square,
        iconSize: _maximized ? 13 : 15,
        onPressed: () => _maximized ? windowManager.unmaximize() : windowManager.maximize(),
      ),
      _WindowButton(tooltip: '关闭', icon: Icons.close, onPressed: windowManager.close, hover: scheme.error, hoverForeground: Colors.white),
    ]);
  }
}

class _WindowButton extends StatefulWidget {
  const _WindowButton({required this.tooltip, required this.icon, required this.onPressed, this.iconSize = 16, this.hover, this.hoverForeground});
  final String tooltip;
  final IconData icon;
  final double iconSize;
  final VoidCallback onPressed;
  final Color? hover, hoverForeground;
  @override
  State<_WindowButton> createState() => _WindowButtonState();
}

class _WindowButtonState extends State<_WindowButton> {
  bool _hovered = false;
  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final active = _hovered && widget.hoverForeground != null;
    return Tooltip(
      message: widget.tooltip,
      waitDuration: const Duration(milliseconds: 600),
      child: MouseRegion(
        onEnter: (_) => setState(() => _hovered = true),
        onExit: (_) => setState(() => _hovered = false),
        child: GestureDetector(
          onTap: widget.onPressed,
          child: Container(
            width: 46,
            color: _hovered ? (widget.hover ?? scheme.muted) : Colors.transparent,
            alignment: Alignment.center,
            child: Icon(widget.icon, size: widget.iconSize, color: active ? widget.hoverForeground : scheme.mutedForeground),
          ),
        ),
      ),
    );
  }
}

/// A slim draggable strip with window buttons, for pages without their own header.
class WindowBar extends StatelessWidget {
  const WindowBar({super.key});
  @override
  Widget build(BuildContext context) {
    if (!customFrame) return const SizedBox.shrink();
    return const WindowDrag(child: SizedBox(height: 36, child: Row(crossAxisAlignment: CrossAxisAlignment.stretch, children: [Spacer(), WindowButtons()])));
  }
}
