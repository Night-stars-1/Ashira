import 'package:flutter/material.dart';

import 'theme.dart';

/// A compact select like the web's shadcn `Select`: an outlined trigger with a
/// chevron and a menu as wide as the trigger (`min-w-(--reka-select-trigger-width)`),
/// 28px rows and a check pinned to the right of the current one.
class SelectField<T> extends StatelessWidget {
  const SelectField({super.key, required this.value, required this.options, required this.onChanged, this.enabled = true, this.label});
  final T value;
  final List<(T, String)> options;
  final ValueChanged<T> onChanged;
  final bool enabled;

  /// Accessible name for the trigger.
  final String? label;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final current = options.where((option) => option.$1 == value).firstOrNull?.$2 ?? '';
    return LayoutBuilder(builder: (context, constraints) {
      // The menu's own 4px padding on each side; rows fill the rest of the trigger's width.
      final rowWidth = constraints.hasBoundedWidth ? (constraints.maxWidth - 8).clamp(96.0, double.infinity) : null;
      return MenuAnchor(
        alignmentOffset: const Offset(0, 4),
        style: MenuStyle(maximumSize: WidgetStatePropertyAll(Size(double.infinity, MediaQuery.sizeOf(context).height * .6))),
        menuChildren: [
          for (final (option, text) in options)
            MenuItemButton(
              onPressed: () => onChanged(option),
              // 28px like the web's `py-1` rows; a little taller for fingers on touch screens.
              style: MenuItemButton.styleFrom(
                minimumSize: Size(0, isDesktop ? 28 : 36),
                padding: const EdgeInsets.only(left: 6, right: 8),
                visualDensity: VisualDensity.standard,
                tapTargetSize: MaterialTapTargetSize.shrinkWrap,
              ),
              child: SizedBox(
                width: rowWidth == null ? null : rowWidth - 14,
                child: Row(children: [
                  Expanded(child: Text(text, maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 14))),
                  const SizedBox(width: 8),
                  SizedBox(width: 16, child: option == value ? const Icon(Icons.check, size: 16) : null),
                ]),
              ),
            ),
        ],
        builder: (context, controller, _) => Semantics(
          label: label,
          button: true,
          child: OutlinedButton(
            onPressed: enabled ? () => controller.isOpen ? controller.close() : controller.open() : null,
            style: OutlinedButton.styleFrom(padding: const EdgeInsets.only(left: 10, right: 6)),
            child: Row(children: [
              Expanded(child: Text(current, maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontWeight: FontWeight.normal))),
              Icon(Icons.expand_more, size: 16, color: scheme.mutedForeground),
            ]),
          ),
        ),
      );
    });
  }
}
