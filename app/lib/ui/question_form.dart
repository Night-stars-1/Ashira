import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../api/models.dart';
import 'theme.dart';

/// An `ask_user` group shown in place of the composer's input.
/// Choices toggle; text fields start with the model's suggested answer.
class QuestionForm extends StatefulWidget {
  const QuestionForm({super.key, required this.request, required this.onAnswer, required this.onInterrupt});
  final QuestionRequest request;
  final Future<void> Function(Map<String, Object?>? answers) onAnswer;
  final VoidCallback onInterrupt;

  @override
  State<QuestionForm> createState() => _QuestionFormState();
}

class _QuestionFormState extends State<QuestionForm> {
  final _picked = <String, List<String>>{};
  final _text = <String, TextEditingController>{};

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void didUpdateWidget(QuestionForm old) {
    super.didUpdateWidget(old);
    if (old.request.id != widget.request.id) {
      _dispose();
      _load();
    }
  }

  @override
  void dispose() {
    _dispose();
    super.dispose();
  }

  void _load() {
    for (final item in widget.request.questions) {
      final text = item.kind == QuestionKind.text && item.defaults.isNotEmpty ? item.defaults.first : '';
      _picked[item.id] = item.kind == QuestionKind.text ? [] : item.defaults.where((d) => item.options.any((o) => o.label == d)).toList();
      _text[item.id] = TextEditingController(text: text)..addListener(() => setState(() {}));
    }
  }

  void _dispose() {
    for (final controller in _text.values) {
      controller.dispose();
    }
    _text.clear();
    _picked.clear();
  }

  void _toggle(QuestionItem item, String option) {
    setState(() {
      final picked = _picked[item.id]!;
      if (item.kind == QuestionKind.multi) {
        picked.contains(option) ? picked.remove(option) : picked.add(option);
      } else {
        final on = !picked.contains(option);
        picked.clear();
        if (on) picked.add(option);
        _text[item.id]!.clear();
      }
    });
  }

  Object? _answerOf(QuestionItem item) {
    final typed = _text[item.id]!.text.trim();
    final picked = _picked[item.id]!;
    switch (item.kind) {
      case QuestionKind.text:
        return typed.isEmpty ? null : typed;
      case QuestionKind.single:
        return typed.isNotEmpty ? typed : picked.firstOrNull;
      case QuestionKind.multi:
        final values = [...picked, if (typed.isNotEmpty && !picked.contains(typed)) typed];
        return values.isEmpty ? null : values;
    }
  }

  Map<String, Object?> get _answers => {for (final item in widget.request.questions) item.id: _answerOf(item)};
  int get _answered => _answers.values.where((value) => value != null).length;

  void _submit() {
    if (_answered > 0) widget.onAnswer(_answers);
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final questions = widget.request.questions;
    final size = MediaQuery.sizeOf(context);
    // Tailwind `sm:`: options with notes sit two per row from 640px up.
    final twoColumns = size.width >= 640;
    // The web's `size="sm"` buttons: 28px tall, 12.8px medium text.
    const small = Size(0, 28);
    const smallText = TextStyle(fontSize: 12.8, fontWeight: FontWeight.w500);
    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.escape): () => widget.onAnswer(null),
        const SingleActivator(LogicalKeyboardKey.enter, control: true): _submit,
        const SingleActivator(LogicalKeyboardKey.enter, meta: true): _submit,
      },
      child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        ConstrainedBox(
          // `max-h-[min(28rem,60dvh)]` for the whole form, footer included.
          constraints: BoxConstraints(maxHeight: (size.height * .6).clamp(0, 448) - 40),
          child: SingleChildScrollView(
            padding: const EdgeInsets.fromLTRB(4, 4, 4, 8),
            child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
              for (final (index, item) in questions.indexed) ...[
                if (index > 0) const SizedBox(height: 12),
                _header(context, item, questions.length > 1 ? index + 1 : null),
                if (item.options.isNotEmpty) ...[
                  const SizedBox(height: 6),
                  Padding(
                    padding: const EdgeInsets.only(left: 24),
                    child: item.options.any((o) => o.description.isNotEmpty)
                        ? _grid([for (final option in item.options) _option(context, item, option)], twoColumns ? 2 : 1)
                        : Wrap(spacing: 6, runSpacing: 6, children: [for (final option in item.options) _option(context, item, option)]),
                  ),
                ],
                const SizedBox(height: 6),
                Padding(
                  padding: const EdgeInsets.only(left: 24),
                  child: TextField(
                    controller: _text[item.id],
                    autofocus: index == 0 && item.kind == QuestionKind.text,
                    minLines: 1,
                    maxLines: 4,
                    style: const TextStyle(fontSize: 14),
                    onChanged: item.kind == QuestionKind.single ? (value) { if (value.trim().isNotEmpty) setState(() => _picked[item.id]!.clear()); } : null,
                    decoration: InputDecoration(
                      isDense: true,
                      // `min-h-9 py-1.5`: a 36px box.
                      contentPadding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                      hintText: switch (item.kind) {
                        QuestionKind.text => '输入回答',
                        QuestionKind.single => '其他（可选，填写后替代所选项）',
                        QuestionKind.multi => '其他（可选，会和勾选项一起提交）',
                      },
                    ),
                  ),
                ),
              ],
            ]),
          ),
        ),
        Divider(height: 1, color: scheme.border),
        Padding(
          padding: const EdgeInsets.fromLTRB(4, 8, 4, 0),
          child: Row(children: [
            Expanded(
              child: Text(
                isDesktop ? '已答 $_answered/${questions.length} · Ctrl+Enter 提交 · Esc 跳过' : '已答 $_answered/${questions.length}',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(fontSize: 12, color: scheme.mutedForeground),
              ),
            ),
            const SizedBox(width: 8),
            TextButton.icon(
              onPressed: widget.onInterrupt,
              style: TextButton.styleFrom(minimumSize: small, padding: const EdgeInsets.symmetric(horizontal: 10), textStyle: smallText, iconSize: 14),
              icon: const Icon(Icons.crop_square_rounded),
              label: const Text('停止'),
            ),
            const SizedBox(width: 8),
            OutlinedButton(
              onPressed: () => widget.onAnswer(null),
              style: OutlinedButton.styleFrom(minimumSize: small, padding: const EdgeInsets.symmetric(horizontal: 10), textStyle: smallText),
              child: const Text('跳过'),
            ),
            const SizedBox(width: 8),
            FilledButton.icon(
              onPressed: _answered > 0 ? _submit : null,
              style: FilledButton.styleFrom(minimumSize: small, padding: const EdgeInsets.symmetric(horizontal: 10), textStyle: smallText, iconSize: 14),
              icon: const Icon(Icons.keyboard_return_rounded),
              label: const Text('提交'),
            ),
          ]),
        ),
      ]),
    );
  }

  /// CSS `grid gap-1.5`: equal-width columns, each row as tall as its tallest card.
  Widget _grid(List<Widget> cells, int columns) => Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        for (var start = 0; start < cells.length; start += columns) ...[
          if (start > 0) const SizedBox(height: 6),
          IntrinsicHeight(
            child: Row(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
              for (var i = start; i < start + columns; i++) ...[
                if (i > start) const SizedBox(width: 6),
                Expanded(child: i < cells.length ? cells[i] : const SizedBox.shrink()),
              ],
            ]),
          ),
        ],
      ]);

  Widget _header(BuildContext context, QuestionItem item, int? number) {
    final scheme = Theme.of(context).colorScheme;
    final kind = switch (item.kind) { QuestionKind.single => '单选', QuestionKind.multi => '多选', QuestionKind.text => '填空' };
    return Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
      Padding(padding: const EdgeInsets.only(top: 2), child: Icon(Icons.help_outline, size: 16, color: scheme.primary, semanticLabel: 'AI 提问')),
      const SizedBox(width: 8),
      Expanded(
        child: Text.rich(TextSpan(children: [
          if (number != null) TextSpan(text: '$number. ', style: TextStyle(color: scheme.mutedForeground, fontFeatures: const [FontFeature.tabularFigures()])),
          TextSpan(text: item.question),
        ]), style: const TextStyle(fontSize: 14, fontWeight: FontWeight.w500, height: 1.45)),
      ),
      const SizedBox(width: 8),
      Container(
        padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
        decoration: BoxDecoration(color: scheme.muted, borderRadius: BorderRadius.circular(4)),
        child: Text(kind, style: TextStyle(fontSize: 12, color: scheme.mutedForeground)),
      ),
    ]);
  }

  /// The web's `size="sm"` option button: `default` when picked, `outline` otherwise.
  Widget _option(BuildContext context, QuestionItem item, QuestionOption option) {
    final on = _picked[item.id]!.contains(option.label);
    final noted = option.description.isNotEmpty;
    final padding = EdgeInsets.symmetric(horizontal: 10, vertical: noted ? 8 : 4);
    final title = Row(mainAxisSize: MainAxisSize.min, children: [
      if (on) const Padding(padding: EdgeInsets.only(right: 6), child: Icon(Icons.check, size: 14)),
      Flexible(child: Text(option.label, style: const TextStyle(fontSize: 12.8, fontWeight: FontWeight.w500))),
    ]);
    final child = noted
        ? Column(crossAxisAlignment: CrossAxisAlignment.start, mainAxisSize: MainAxisSize.min, children: [
            title,
            const SizedBox(height: 2),
            Opacity(opacity: .75, child: Text(option.description, style: const TextStyle(fontSize: 12, fontWeight: FontWeight.w400, height: 1.4))),
          ])
        : title;
    final align = noted ? Alignment.topLeft : null;
    const minimum = Size(0, 28);
    void tap() => _toggle(item, option.label);
    return Semantics(
      checked: on,
      inMutuallyExclusiveGroup: item.kind == QuestionKind.single,
      hint: noted ? option.description : null,
      child: on
          ? FilledButton(style: FilledButton.styleFrom(padding: padding, minimumSize: minimum, alignment: align), onPressed: tap, child: child)
          : OutlinedButton(style: OutlinedButton.styleFrom(padding: padding, minimumSize: minimum, alignment: align), onPressed: tap, child: child),
    );
  }
}
