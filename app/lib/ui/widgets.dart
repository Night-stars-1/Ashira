import 'package:flutter/material.dart';

import 'theme.dart';

/// 48px column header with a bottom border, like the web's `h-12 border-b`.
class PanelHeader extends StatelessWidget {
  const PanelHeader({super.key, required this.children, this.padding = const EdgeInsets.symmetric(horizontal: 8)});
  final List<Widget> children;
  final EdgeInsets padding;
  @override
  Widget build(BuildContext context) => Container(
        height: 48,
        padding: padding,
        decoration: BoxDecoration(border: Border(bottom: BorderSide(color: Theme.of(context).colorScheme.border))),
        child: Row(children: children),
      );
}

/// A compact row with hover highlight, used for lists (sessions, files, providers).
/// Pressing nudges it down 1px, the web Button's `active:translate-y-px`.
class CompactRow extends StatefulWidget {
  const CompactRow({super.key, required this.child, this.onTap, this.onLongPress, this.selected = false, this.padding = const EdgeInsets.symmetric(horizontal: 8)});
  final Widget child;
  final VoidCallback? onTap;
  /// Long press or right click, with where it happened in the row (for a context menu).
  final void Function(Offset position)? onLongPress;
  final bool selected;
  final EdgeInsets padding;
  @override
  State<CompactRow> createState() => _CompactRowState();
}

class _CompactRowState extends State<CompactRow> {
  bool _pressed = false;
  Offset _at = Offset.zero;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final menu = widget.onLongPress;
    return Transform.translate(
      offset: Offset(0, _pressed ? 1 : 0),
      child: Material(
        color: widget.selected ? scheme.muted : Colors.transparent,
        borderRadius: BorderRadius.circular(radius),
        child: InkWell(
          borderRadius: BorderRadius.circular(radius),
          onTap: widget.onTap,
          onTapDown: (details) => _at = details.localPosition,
          onSecondaryTapDown: (details) => _at = details.localPosition,
          onLongPress: menu == null ? null : () => menu(_at),
          onSecondaryTap: menu == null ? null : () => menu(_at),
          onHighlightChanged: (pressed) => setState(() => _pressed = pressed),
          // Padding eases like the web Button's `transition-all` (150ms, Tailwind's ease). The
          // session rows rely on it: on hover the time hides at once while the right padding
          // grows for the 「…」 button, and on leave the time slides back in as it shrinks.
          child: SizedBox(
            height: isDesktop ? 32 : 40,
            child: AnimatedPadding(duration: const Duration(milliseconds: 150), curve: const Cubic(.4, 0, .2, 1), padding: widget.padding, child: widget.child),
          ),
        ),
      ),
    );
  }
}

/// The web's shadcn switch: 32×18px track, 16px thumb. Material's is 52×32.
class Toggle extends StatelessWidget {
  const Toggle({super.key, required this.value, required this.onChanged, this.label});
  final bool value;
  final ValueChanged<bool>? onChanged;
  final String? label;
  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final enabled = onChanged != null;
    // The web's `bg-input` (the field border color in this theme); the dark thumb is foreground.
    final off = (Theme.of(context).inputDecorationTheme.enabledBorder?.borderSide.color ?? scheme.outlineVariant);
    return Semantics(
      toggled: value,
      enabled: enabled,
      label: label,
      child: MouseRegion(
        cursor: enabled ? SystemMouseCursors.click : SystemMouseCursors.basic,
        child: GestureDetector(
          onTap: enabled ? () => onChanged!(!value) : null,
          child: Opacity(
            opacity: enabled ? 1 : .5,
            child: AnimatedContainer(
              duration: const Duration(milliseconds: 150),
              width: 32,
              height: 18,
              padding: const EdgeInsets.all(1),
              alignment: value ? Alignment.centerRight : Alignment.centerLeft,
              decoration: BoxDecoration(color: value ? scheme.primary : off, borderRadius: BorderRadius.circular(9)),
              child: Container(
                width: 16,
                height: 16,
                decoration: BoxDecoration(shape: BoxShape.circle, color: value ? scheme.onPrimary : scheme.onSurface),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// Small toggle button; the selected one is filled, like the web's `default` vs `outline` variants.
class Pill extends StatelessWidget {
  const Pill({super.key, required this.label, required this.selected, required this.onTap});
  final String label;
  final bool selected;
  final VoidCallback onTap;
  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Material(
      color: selected ? scheme.primary : Colors.transparent,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(6), side: BorderSide(color: selected ? scheme.primary : scheme.border)),
      child: InkWell(
        borderRadius: BorderRadius.circular(6),
        onTap: onTap,
        child: SizedBox(
          height: isDesktop ? 28 : 34,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 10),
            child: Center(
              widthFactor: 1,
              child: Text(label, style: TextStyle(fontSize: 12, fontWeight: FontWeight.w500, color: selected ? scheme.onPrimary : scheme.onSurface)),
            ),
          ),
        ),
      ),
    );
  }
}

class StatusTag extends StatelessWidget {
  const StatusTag(this.text, {super.key, this.destructive = false, this.accent = false});
  final String text;
  final bool destructive;
  /// Calls for action (an available update): primary fill.
  final bool accent;
  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final (background, foreground) = destructive
        ? (scheme.error.withValues(alpha: .15), scheme.error)
        : accent
            ? (scheme.primary, scheme.onPrimary)
            : (scheme.muted, scheme.onSurface);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
      decoration: BoxDecoration(color: background, borderRadius: BorderRadius.circular(6)),
      child: Text(text, style: TextStyle(fontSize: 12, fontWeight: FontWeight.w500, color: foreground)),
    );
  }
}

/// Rounded bordered box; with [divided] the children are separated by hairlines.
class Bordered extends StatelessWidget {
  const Bordered({super.key, required this.children, this.divided = true, this.padding = EdgeInsets.zero, this.tinted = false});
  final List<Widget> children;
  final bool divided, tinted;
  final EdgeInsets padding;
  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Container(
      padding: padding,
      decoration: BoxDecoration(
        color: tinted ? scheme.muted.withValues(alpha: .3) : null,
        border: Border.all(color: scheme.border),
        borderRadius: BorderRadius.circular(radius),
      ),
      clipBehavior: Clip.antiAlias,
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        for (final (index, child) in children.indexed) ...[
          if (divided && index > 0) Divider(color: scheme.border),
          child,
        ],
      ]),
    );
  }
}

/// A settings group's card, Chrome-style: filled, rounded, no border, rows split by a
/// hairline; it sits under a small heading on the page background.
class SettingsCard extends StatelessWidget {
  const SettingsCard({super.key, required this.children});
  final List<Widget> children;
  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return DecoratedBox(
      decoration: BoxDecoration(color: scheme.muted.withValues(alpha: .6), borderRadius: BorderRadius.circular(radius + 4)),
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        for (final (index, child) in children.indexed) ...[
          // 60% of the border's own opacity, like the web's `divide-border/60`; `withValues(alpha:)`
          // alone replaces it, which made the dark theme's 10% white line 60% white.
          if (index > 0) Divider(height: 1, color: scheme.border.withValues(alpha: scheme.border.a * .6)),
          Padding(padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14), child: child),
        ],
      ]),
    );
  }
}

/// Label on the left, value on the right; a row inside [Bordered].
class InfoRow extends StatelessWidget {
  const InfoRow(this.label, this.value, {super.key});
  final String label, value;
  @override
  Widget build(BuildContext context) => Padding(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 9),
        child: Row(children: [
          Expanded(child: Text(label, style: TextStyle(fontSize: 13, color: Theme.of(context).colorScheme.mutedForeground))),
          const SizedBox(width: 12),
          Text(value, style: const TextStyle(fontSize: 13, fontFeatures: [FontFeature.tabularFigures()])),
        ]),
      );
}

class SectionTitle extends StatelessWidget {
  const SectionTitle(this.text, {super.key, this.note});
  final String text;
  final String? note;
  @override
  Widget build(BuildContext context) => Padding(
        padding: const EdgeInsets.only(bottom: 8),
        child: Text.rich(TextSpan(children: [
          TextSpan(text: text, style: const TextStyle(fontSize: 14, fontWeight: FontWeight.w600)),
          if (note != null) TextSpan(text: '  $note', style: TextStyle(fontSize: 12, color: Theme.of(context).colorScheme.mutedForeground)),
        ])),
      );
}

class EmptyState extends StatelessWidget {
  const EmptyState({super.key, required this.title, required this.description});
  final String title, description;
  @override
  Widget build(BuildContext context) => Container(
        padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 40),
        decoration: BoxDecoration(border: Border.all(color: Theme.of(context).colorScheme.border), borderRadius: BorderRadius.circular(radius)),
        child: Column(children: [
          Text(title, style: const TextStyle(fontSize: 14, fontWeight: FontWeight.w600)),
          const SizedBox(height: 6),
          Text(description, textAlign: TextAlign.center, style: TextStyle(fontSize: 13, color: Theme.of(context).colorScheme.mutedForeground)),
        ]),
      );
}
