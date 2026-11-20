import 'package:flutter/material.dart';

import 'theme.dart';

/// A compact select like the web's shadcn `Select`: an outlined trigger with a
/// chevron and a menu of options with a check on the current one.
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
    return MenuAnchor(
      alignmentOffset: const Offset(0, 4),
      menuChildren: [
        for (final (option, text) in options)
          MenuItemButton(
            onPressed: () => onChanged(option),
            trailingIcon: option == value ? const Icon(Icons.check, size: 16) : const SizedBox(width: 16),
            child: ConstrainedBox(constraints: const BoxConstraints(minWidth: 96), child: Text(text)),
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
  }
}
