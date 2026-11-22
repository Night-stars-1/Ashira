import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:window_manager/window_manager.dart';

import 'theme.dart';

/// Windows and Linux draw their own title bar: the OS frame is hidden, the
/// app's header rows double as the drag area, and window buttons sit at the
/// end of the top-right header.
bool get customFrame => !kIsWeb && (defaultTargetPlatform == TargetPlatform.windows || defaultTargetPlatform == TargetPlatform.linux);

/// macOS hides only the title bar's background: the app fills the window and the
/// native traffic lights sit centered in its top-left header (MainFlutterWindow.swift).
bool get macFrame => !kIsWeb && defaultTargetPlatform == TargetPlatform.macOS;

/// Desktop windows open panels as dialogs at any width; full-screen pages would
/// cover the window buttons (Windows, Linux) or slide under the traffic lights (macOS).
bool get desktopWindow => customFrame || macFrame;

/// Width the traffic lights take from the window's left edge, gap after them included.
double _trafficLights = 0;
final windowMaximized = ValueNotifier(false);
final windowFullScreen = ValueNotifier(false);

Future<void> setupWindow() async {
  if (!customFrame && !macFrame) return;
  await windowManager.ensureInitialized();
  await windowManager.setTitle('Ashira');
  await windowManager.setMinimumSize(const Size(560, 420));
  if (customFrame) {
    await windowManager.setTitleBarStyle(TitleBarStyle.hidden);
  } else {
    _trafficLights = await const MethodChannel('ashira/window').invokeMethod<double>('trafficLightsWidth') ?? 0;
  }
  windowManager.addListener(_WindowState());
  windowMaximized.value = await windowManager.isMaximized();
  windowFullScreen.value = await windowManager.isFullScreen();
}

class _WindowState with WindowListener {
  @override
  void onWindowMaximize() => windowMaximized.value = true;
  @override
  void onWindowUnmaximize() => windowMaximized.value = false;
  @override
  void onWindowEnterFullScreen() => windowFullScreen.value = true;
  @override
  void onWindowLeaveFullScreen() => windowFullScreen.value = false;
}

/// Drags the window, double click maximizes, right click opens the system menu;
/// buttons inside stay clickable.
class WindowDrag extends StatelessWidget {
  const WindowDrag({super.key, required this.child});
  final Widget child;
  @override
  Widget build(BuildContext context) => customFrame || macFrame ? DragToMoveArea(child: child) : child;
}

/// Start of a header row in the window's top-left corner: room for the macOS
/// traffic lights (none in full screen, where they hide), else [normal].
class TrafficLightsSpace extends StatelessWidget {
  const TrafficLightsSpace({super.key, required this.normal, this.topLeft = true});
  final double normal;
  /// False when another header holds the corner; then this is just [normal].
  final bool topLeft;
  @override
  Widget build(BuildContext context) {
    if (!macFrame || !topLeft) return SizedBox(width: normal);
    return ValueListenableBuilder(valueListenable: windowFullScreen, builder: (_, full, _) => SizedBox(width: full ? normal : _trafficLights));
  }
}

/// Linux windows have no OS frame on most desktops once the title bar is hidden;
/// these edges resize them. The tree stays the same when maximized so app state survives.
class WindowResizeEdges extends StatelessWidget {
  const WindowResizeEdges({super.key, required this.child});
  final Widget child;
  @override
  Widget build(BuildContext context) {
    if (kIsWeb || defaultTargetPlatform != TargetPlatform.linux) return child;
    return ListenableBuilder(
      listenable: Listenable.merge([windowMaximized, windowFullScreen]),
      builder: (_, child) => DragToResizeArea(
        resizeEdgeSize: 6,
        enableResizeEdges: windowMaximized.value || windowFullScreen.value ? const [] : null,
        child: child!,
      ),
      child: child,
    );
  }
}

/// Minimize, maximize/restore and close; full height of the row they end.
class WindowButtons extends StatelessWidget {
  const WindowButtons({super.key});

  @override
  Widget build(BuildContext context) {
    if (!customFrame) return const SizedBox.shrink();
    final scheme = Theme.of(context).colorScheme;
    return ValueListenableBuilder(
      valueListenable: windowMaximized,
      builder: (_, maximized, _) => Row(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        _WindowButton(tooltip: '最小化', icon: Icons.remove, onPressed: windowManager.minimize),
        _WindowButton(
          tooltip: maximized ? '还原' : '最大化',
          icon: maximized ? Icons.filter_none : Icons.crop_square,
          iconSize: maximized ? 13 : 15,
          onPressed: () => maximized ? windowManager.unmaximize() : windowManager.maximize(),
        ),
        _WindowButton(tooltip: '关闭', icon: Icons.close, onPressed: windowManager.close, hover: scheme.error, hoverForeground: Colors.white),
      ]),
    );
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

/// A slim draggable strip for pages without their own header: window buttons on
/// Windows and Linux, room under the traffic lights on macOS.
class WindowBar extends StatelessWidget {
  const WindowBar({super.key});
  @override
  Widget build(BuildContext context) {
    if (macFrame) return const WindowDrag(child: SizedBox(height: 36));
    if (!customFrame) return const SizedBox.shrink();
    return const WindowDrag(child: SizedBox(height: 36, child: Row(crossAxisAlignment: CrossAxisAlignment.stretch, children: [Spacer(), WindowButtons()])));
  }
}
