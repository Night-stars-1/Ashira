import 'package:flutter/material.dart';

import '../api/models.dart';
import 'dialogs.dart';
import 'theme.dart';

/// A small card for an attached text file; tapping shows the whole text.
class TextFileCard extends StatelessWidget {
  const TextFileCard({super.key, required this.file, this.onRemove});
  final TextFile file;
  final VoidCallback? onRemove;

  /// Compact counts so each fits the card; exact numbers are in the tooltip.
  static String _count(int n) => n >= 10000 ? '${(n / 10000).toStringAsFixed(1).replaceFirst(RegExp(r'\.0$'), '')}万' : '$n';

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final size = '${file.lines} 行 · ${file.text.length} 字';
    final small = TextStyle(fontSize: 11, height: 1.2, color: scheme.mutedForeground);
    return SizedBox(
      width: 96,
      child: Stack(children: [
        Material(
          color: scheme.muted,
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(6), side: BorderSide(color: scheme.border)),
          clipBehavior: Clip.antiAlias,
          child: InkWell(
            onTap: () => showPanel(
              context,
              title: file.name,
              description: size,
              width: 760,
              builder: (_) => SingleChildScrollView(
                padding: const EdgeInsets.all(16),
                child: SelectableText(file.text, style: monoStyle(context).copyWith(fontSize: 12, height: 1.5)),
              ),
            ),
            child: Tooltip(
              message: '${file.name}\n$size',
              waitDuration: const Duration(milliseconds: 600),
              child: Padding(
                padding: const EdgeInsets.all(8),
                child: Column(crossAxisAlignment: CrossAxisAlignment.start, mainAxisSize: MainAxisSize.min, children: [
                  Icon(Icons.description_outlined, size: 20, color: scheme.mutedForeground),
                  const SizedBox(height: 4),
                  Text(file.name, maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 12, fontWeight: FontWeight.w500)),
                  Text('${_count(file.lines)} 行', maxLines: 1, style: small),
                  Text('${_count(file.text.length)} 字', maxLines: 1, style: small),
                ]),
              ),
            ),
          ),
        ),
        if (onRemove != null)
          Positioned(
            right: 4,
            top: 4,
            child: InkWell(
              onTap: onRemove,
              child: CircleAvatar(radius: 10, backgroundColor: scheme.surface, child: Icon(Icons.close, size: 12, semanticLabel: '移除文件 ${file.name}')),
            ),
          ),
      ]),
    );
  }
}
