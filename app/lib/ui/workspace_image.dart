import 'package:flutter/material.dart';

import 'image_viewer.dart';
import 'theme.dart';

/// Maps a workspace-relative image path to its `/raw` URL for the session shown below it.
class WorkspaceImages extends InheritedWidget {
  const WorkspaceImages({super.key, required this.uri, required super.child});
  final Uri Function(String path) uri;

  static WorkspaceImages? of(BuildContext context) => context.dependOnInheritedWidgetOfExactType<WorkspaceImages>();

  @override
  bool updateShouldNotify(WorkspaceImages old) => old.uri != uri;
}

/// A Markdown image target that names a workspace file: no scheme, not protocol-relative.
bool isWorkspacePath(String src) => src.isNotEmpty && !RegExp(r'^[a-zA-Z][a-zA-Z0-9+.-]*:').hasMatch(src) && !src.startsWith('//');

/// An image file in the session's workspace; a tap shows it full size.
class WorkspaceImage extends StatelessWidget {
  const WorkspaceImage(this.path, {super.key, this.maxHeight = 384});
  final String path;
  final double maxHeight;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final uri = WorkspaceImages.of(context)?.uri(path);
    Widget failed() => Container(
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
          decoration: BoxDecoration(border: Border.all(color: scheme.border), borderRadius: BorderRadius.circular(6)),
          child: Text('图片无法加载：$path', style: TextStyle(fontSize: 12, color: scheme.mutedForeground)),
        );
    if (uri == null) return failed();
    final url = uri.toString();
    return Tooltip(
      message: path,
      child: InkWell(
        borderRadius: BorderRadius.circular(radius),
        onTap: () => showImageViewer(context, NetworkImage(url), label: path),
        child: ConstrainedBox(
          constraints: BoxConstraints(maxHeight: maxHeight),
          child: ClipRRect(
            borderRadius: BorderRadius.circular(radius),
            child: Image.network(
              url,
              fit: BoxFit.contain,
              errorBuilder: (_, _, _) => failed(),
              loadingBuilder: (context, child, progress) => progress == null
                  ? child
                  : SizedBox.square(dimension: 64, child: Center(child: SizedBox.square(dimension: 16, child: CircularProgressIndicator(strokeWidth: 1.5, color: scheme.mutedForeground)))),
            ),
          ),
        ),
      ),
    );
  }
}
