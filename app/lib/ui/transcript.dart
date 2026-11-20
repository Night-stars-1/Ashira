import 'dart:convert';

import 'package:flutter/material.dart';

import '../api/models.dart';
import 'markdown.dart';
import 'text_file_card.dart';
import 'theme.dart';

/// Scrollable chat log. Follows new content while the user is at the bottom.
class Transcript extends StatefulWidget {
  const Transcript({super.key, required this.blocks, this.activity = '', this.onOpenAgent, this.emptyText});
  final List<Block> blocks;
  final String activity;
  final void Function(String id)? onOpenAgent;
  final String? emptyText;

  @override
  State<Transcript> createState() => _TranscriptState();
}

class _TranscriptState extends State<Transcript> {
  final _scroll = ScrollController();
  bool _follow = true;

  @override
  void initState() {
    super.initState();
    _scroll.addListener(() {
      if (!_scroll.hasClients) return;
      final position = _scroll.position;
      final follow = position.pixels >= position.maxScrollExtent - 48;
      // Rebuild only when the 「回到底部」 button should appear or go.
      if (follow != _follow) setState(() => _follow = follow);
    });
    _pin();
  }

  @override
  void didUpdateWidget(Transcript oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (_follow) _pin();
  }

  void _pin() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_scroll.hasClients) _scroll.jumpTo(_scroll.position.maxScrollExtent);
    });
  }

  void _toBottom() {
    if (!_scroll.hasClients) return;
    _scroll.animateTo(_scroll.position.maxScrollExtent, duration: const Duration(milliseconds: 250), curve: Curves.easeOut);
    setState(() => _follow = true);
  }

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final count = widget.blocks.length + (widget.activity.isEmpty ? 0 : 1);
    if (count == 0) {
      return Center(child: Text(widget.emptyText ?? '没有对话', style: TextStyle(color: Theme.of(context).colorScheme.onSurfaceVariant)));
    }
    final wide = MediaQuery.sizeOf(context).width >= 640;
    final scheme = Theme.of(context).colorScheme;
    return Stack(children: [
      Scrollbar(
        controller: _scroll,
        child: ListView.builder(
          controller: _scroll,
          padding: EdgeInsets.symmetric(horizontal: wide ? 24 : 12, vertical: wide ? 24 : 16),
          itemCount: count,
          itemBuilder: (context, index) {
            final child = index < widget.blocks.length
                ? _BlockView(block: widget.blocks[index], onOpenAgent: widget.onOpenAgent)
                : _Activity(widget.activity);
            return Center(
              child: ConstrainedBox(constraints: const BoxConstraints(maxWidth: 768), child: Padding(padding: const EdgeInsets.only(bottom: 16), child: child)),
            );
          },
        ),
      ),
      if (!_follow)
        Positioned(
          bottom: 12,
          left: 0,
          right: 0,
          child: Center(
            child: Tooltip(
              message: '回到底部',
              child: Material(
                color: scheme.card,
                shape: CircleBorder(side: BorderSide(color: scheme.border)),
                elevation: 2,
                child: InkWell(customBorder: const CircleBorder(), onTap: _toBottom, child: const Padding(padding: EdgeInsets.all(8), child: Icon(Icons.arrow_downward, size: 18))),
              ),
            ),
          ),
        ),
    ]);
  }
}

class _Activity extends StatelessWidget {
  const _Activity(this.text);
  final String text;
  @override
  Widget build(BuildContext context) => Row(children: [
        SizedBox.square(dimension: 14, child: CircularProgressIndicator(strokeWidth: 1.5, color: Theme.of(context).colorScheme.mutedForeground)),
        const SizedBox(width: 8),
        Text(text, style: TextStyle(fontSize: 14, color: Theme.of(context).colorScheme.mutedForeground)),
      ]);
}

class _BlockView extends StatelessWidget {
  const _BlockView({required this.block, this.onOpenAgent});
  final Block block;
  final void Function(String id)? onOpenAgent;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    switch (block) {
      case UserBlock(:final text, :final images, :final files):
        return Align(
          alignment: Alignment.centerRight,
          child: FractionallySizedBox(
            widthFactor: 0.85,
            alignment: Alignment.centerRight,
            child: Column(crossAxisAlignment: CrossAxisAlignment.end, children: [
              for (final src in images) _DataImage(src),
              if (files.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(bottom: 8),
                  child: Wrap(alignment: WrapAlignment.end, spacing: 8, runSpacing: 8, children: [for (final file in files) TextFileCard(file: file)]),
                ),
              if (text.isNotEmpty)
                Container(
                  padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
                  decoration: BoxDecoration(color: scheme.primary, borderRadius: BorderRadius.circular(16)),
                  child: SelectableText(text, style: TextStyle(fontSize: 14, color: scheme.onPrimary, height: 1.5)),
                ),
            ]),
          ),
        );
      case NoteBlock(:final text, :final error, :final agent):
        final color = error ? scheme.error : scheme.mutedForeground;
        final body = Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
          if (agent != null) Padding(padding: const EdgeInsets.only(right: 8, top: 2), child: Icon(Icons.smart_toy_outlined, size: 16, color: color)),
          Expanded(child: Text(text, style: TextStyle(fontSize: 14, color: color, height: 1.5))),
          if (agent != null) Text('查看', style: TextStyle(fontSize: 14, color: color, decoration: TextDecoration.underline, decorationColor: color)),
        ]);
        return Material(
          color: Colors.transparent,
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(radius), side: BorderSide(color: error ? scheme.error.withValues(alpha: .4) : scheme.border)),
          clipBehavior: Clip.antiAlias,
          child: InkWell(
            onTap: agent == null || onOpenAgent == null ? null : () => onOpenAgent!(agent),
            child: Padding(padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8), child: body),
          ),
        );
      case AssistantBlock(:final parts):
        return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
          for (final part in parts)
            Padding(
              padding: const EdgeInsets.only(bottom: 8),
              child: switch (part) {
                TextPart(reasoning: true, :final text) => _Reasoning(text),
                TextPart(:final text) => Markdown(text),
                ToolPart() => ToolCard(part),
              },
            ),
        ]);
    }
  }
}

/// The web's reasoning block: short thoughts show inline; long ones (over 6 lines
/// or 400 characters) start collapsed to a one-line preview with a length.
class _Reasoning extends StatefulWidget {
  const _Reasoning(this.text);
  final String text;
  @override
  State<_Reasoning> createState() => _ReasoningState();
}

class _ReasoningState extends State<_Reasoning> {
  /// Flips the default: long ones open, short ones close.
  bool _toggled = false;

  bool get _long => widget.text.length > 400 || '\n'.allMatches(widget.text).length >= 6;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final style = TextStyle(fontSize: 14, fontStyle: FontStyle.italic, color: scheme.mutedForeground, height: 1.5);
    final box = BoxDecoration(color: scheme.muted, borderRadius: BorderRadius.circular(radius));
    if (!_long) {
      return Container(padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8), decoration: box, child: Text(widget.text, style: style));
    }
    final open = _toggled;
    return Container(
      decoration: box,
      clipBehavior: Clip.antiAlias,
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        InkWell(
          onTap: () => setState(() => _toggled = !_toggled),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
            child: Row(children: [
              AnimatedRotation(turns: open ? .25 : 0, duration: const Duration(milliseconds: 150), child: Icon(Icons.chevron_right, size: 14, color: scheme.mutedForeground)),
              const SizedBox(width: 6),
              Text('思考', style: TextStyle(fontSize: 14, fontWeight: FontWeight.w500, color: scheme.mutedForeground)),
              const SizedBox(width: 6),
              Expanded(
                child: open
                    ? const SizedBox.shrink()
                    : Text(widget.text.replaceAll(RegExp(r'\s+'), ' ').trim(), maxLines: 1, overflow: TextOverflow.ellipsis, style: style.copyWith(color: scheme.mutedForeground.withValues(alpha: .75))),
              ),
              const SizedBox(width: 6),
              Text('${widget.text.length} 字', style: TextStyle(fontSize: 12, color: scheme.mutedForeground.withValues(alpha: .75), fontFeatures: const [FontFeature.tabularFigures()])),
            ]),
          ),
        ),
        AnimatedSize(
          duration: const Duration(milliseconds: 150),
          alignment: Alignment.topCenter,
          child: open ? Padding(padding: const EdgeInsets.fromLTRB(12, 0, 12, 8), child: Text(widget.text, style: style)) : const SizedBox(width: double.infinity),
        ),
      ]),
    );
  }
}

class _DataImage extends StatelessWidget {
  const _DataImage(this.src);
  final String src;
  @override
  Widget build(BuildContext context) {
    final comma = src.indexOf(',');
    if (!src.startsWith('data:') || comma < 0) return const SizedBox.shrink();
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: ClipRRect(
        borderRadius: BorderRadius.circular(12),
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxHeight: 240),
          child: Image.memory(base64Decode(src.substring(comma + 1)), fit: BoxFit.contain, gaplessPlayback: true),
        ),
      ),
    );
  }
}

/// The web's ToolOutput: a badge with the tool's name and its summary, then the result
/// (todo board, `ask_user` answers or raw/numbered output) under a divider. Always open.
class ToolCard extends StatelessWidget {
  const ToolCard(this.tool, {super.key});
  final ToolPart tool;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final todo = tool.name == 'todo' ? parseTodo(tool.output) : null;
    final asked = tool.name == 'ask_user' ? parseAsk(tool.output) : null;
    final numbered = todo == null && asked == null && tool.output.isNotEmpty ? numberedLines(tool.output) : null;
    final label = switch (tool.name) { 'todo' => '待办', 'ask_user' => '提问', _ => tool.name };
    // `max-h-64` scroll area for the todo board and raw output; answers are short and show whole.
    Widget scrolled(Widget child) => ConstrainedBox(
          constraints: const BoxConstraints(maxHeight: 256),
          child: SingleChildScrollView(padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8), child: child),
        );
    final Widget? body = todo != null
        ? scrolled(_TodoBoard(todo))
        : asked != null
            ? Padding(padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8), child: _AskResult(asked))
            : tool.output.isEmpty
                ? null
                : scrolled(numbered != null ? _Numbered(numbered) : SelectableText(tool.output, style: monoStyle(context).copyWith(fontSize: 12, height: 1.45)));
    return Container(
      decoration: BoxDecoration(border: Border.all(color: scheme.border), borderRadius: BorderRadius.circular(radius)),
      clipBehavior: Clip.antiAlias,
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
          child: Row(children: [
            // The web's secondary Badge: 20px tall, fully rounded, 12px medium text.
            Container(
              height: 20,
              padding: const EdgeInsets.symmetric(horizontal: 8),
              alignment: Alignment.center,
              decoration: BoxDecoration(color: scheme.secondary, borderRadius: BorderRadius.circular(999)),
              child: Text(label, style: TextStyle(fontSize: 12, fontWeight: FontWeight.w500, height: 1, color: scheme.onSecondary)),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: Text(tool.name == 'todo' ? '' : tool.summary, maxLines: 1, overflow: TextOverflow.ellipsis, style: TextStyle(color: scheme.mutedForeground, fontSize: 14)),
            ),
            if (!tool.done)
              Text('运行中', style: TextStyle(fontSize: 12, color: scheme.mutedForeground))
            else if (tool.error)
              Text('失败', style: TextStyle(fontSize: 12, color: scheme.error)),
          ]),
        ),
        if (body != null) ...[const Divider(), body],
      ]),
    );
  }
}

typedef AskAnswer = ({String question, String answer, bool typed, bool skipped});

/// Parses the `ask_user` result; null falls back to the raw text. An empty list means the group was skipped.
List<AskAnswer>? parseAsk(String output) {
  if (output.startsWith('The user skipped')) return const [];
  final lines = output.split('\n');
  if (lines.first != 'The user answered:') return null;
  final answers = <AskAnswer>[];
  for (var i = 1; i + 1 < lines.length; i += 2) {
    final question = RegExp(r'^\d+\. (.*)$').firstMatch(lines[i])?[1];
    final answer = RegExp(r'^ {3}→ (.*)$').firstMatch(lines[i + 1])?[1];
    if (question == null || answer == null) return null;
    const marker = ' (typed by the user instead of an option)';
    final skipped = answer.startsWith('(no answer');
    answers.add((question: question, typed: answer.contains(marker), skipped: skipped, answer: skipped ? '' : answer.replaceAll(marker, '').replaceAll('; ', '、')));
  }
  return answers.isEmpty ? null : answers;
}

class _AskResult extends StatelessWidget {
  const _AskResult(this.answers);
  final List<AskAnswer> answers;
  @override
  Widget build(BuildContext context) {
    final muted = Theme.of(context).colorScheme.mutedForeground;
    if (answers.isEmpty) return Text('已跳过，由 AI 自行判断', style: TextStyle(fontSize: 14, color: muted));
    // The web's `<dl>`: 10px between questions, 4px between a question and its answer.
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      for (final (index, item) in answers.indexed) ...[
        if (index > 0) const SizedBox(height: 10),
        Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Padding(padding: const EdgeInsets.only(top: 3, right: 6), child: Icon(Icons.help_outline, size: 14, color: muted)),
          Expanded(child: Text(item.question, style: TextStyle(fontSize: 14, color: muted))),
        ]),
        const SizedBox(height: 4),
        Padding(
          padding: const EdgeInsets.only(left: 20),
          child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
            Padding(padding: const EdgeInsets.only(top: 3, right: 6), child: Icon(Icons.subdirectory_arrow_right, size: 14, color: muted)),
            Expanded(
              child: item.skipped
                  ? Text('未回答', style: TextStyle(fontSize: 14, color: muted))
                  : Text.rich(TextSpan(children: [
                      TextSpan(text: item.answer, style: const TextStyle(fontSize: 14, fontWeight: FontWeight.w500)),
                      if (item.typed) TextSpan(text: '  （自填）', style: TextStyle(fontSize: 12, color: muted)),
                    ])),
            ),
          ]),
        ),
      ],
    ]);
  }
}

/// `read` output with `N<TAB>line` rows; the numbers become a gutter.
List<({String n, String text})>? numberedLines(String output) {
  final rows = <({String n, String text})>[];
  var saw = false;
  for (final line in output.split('\n')) {
    final match = RegExp(r'^ *(\d+)\t(.*)$').firstMatch(line);
    if (match != null) {
      saw = true;
      rows.add((n: match[1]!, text: match[2]!));
    } else if (saw && (line.isEmpty || line.startsWith('[lines '))) {
      rows.add((n: '', text: line));
    } else {
      return null;
    }
  }
  return saw ? rows : null;
}

class _Numbered extends StatelessWidget {
  const _Numbered(this.rows);
  final List<({String n, String text})> rows;
  @override
  Widget build(BuildContext context) {
    final mono = monoStyle(context).copyWith(fontSize: 12, height: 1.45);
    final gutter = mono.copyWith(color: Theme.of(context).colorScheme.mutedForeground);
    final width = rows.fold<int>(1, (max, row) => row.n.length > max ? row.n.length : max) * 8.0 + 8;
    return SelectionArea(
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        for (final row in rows)
          Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
            SizedBox(width: width, child: SelectionContainer.disabled(child: Text(row.n, textAlign: TextAlign.right, style: gutter))),
            const SizedBox(width: 12),
            Expanded(child: Text(row.text, style: mono)),
          ]),
      ]),
    );
  }
}

typedef TodoTask = ({String content, String status, String reason});
typedef TodoPhase = ({String name, List<TodoTask> tasks});

List<TodoPhase>? parseTodo(String output) {
  final lines = output.split('\n');
  if (lines.isEmpty || !lines.first.startsWith('Todo: ')) return null;
  final phases = <TodoPhase>[];
  final pattern = RegExp(r'^- (.+) \[(pending|in_progress|completed|abandoned|blocked)\](?: \((.*)\))?$');
  for (final line in lines.skip(1)) {
    if (line.isEmpty) continue;
    if (line.endsWith(':') && !line.startsWith('- ')) {
      phases.add((name: line.substring(0, line.length - 1), tasks: []));
      continue;
    }
    final match = pattern.firstMatch(line);
    if (match == null || phases.isEmpty) return null;
    phases.last.tasks.add((content: match[1]!, status: match[2]!, reason: match[3] ?? ''));
  }
  return phases;
}

class _TodoBoard extends StatelessWidget {
  const _TodoBoard(this.phases);
  final List<TodoPhase> phases;

  static const _labels = {'in_progress': '进行中', 'completed': '完成', 'abandoned': '放弃', 'blocked': '阻塞'};
  static const _icons = {
    'in_progress': Icons.radio_button_checked,
    'completed': Icons.check,
    'abandoned': Icons.remove,
    'blocked': Icons.block,
  };

  @override
  Widget build(BuildContext context) {
    final muted = Theme.of(context).colorScheme.onSurfaceVariant;
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      for (final phase in phases) ...[
        Padding(padding: const EdgeInsets.only(top: 4, bottom: 4), child: Text(phase.name, style: TextStyle(fontSize: 12, fontWeight: FontWeight.w600, color: muted))),
        for (final task in phase.tasks)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 2),
            child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Icon(_icons[task.status] ?? Icons.radio_button_unchecked, size: 15, color: task.status == 'in_progress' || task.status == 'blocked' ? null : muted),
              const SizedBox(width: 8),
              Expanded(
                child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                  Text(task.content,
                      style: TextStyle(
                          fontSize: 13,
                          color: task.status == 'completed' || task.status == 'abandoned' ? muted : null,
                          decoration: task.status == 'completed' || task.status == 'abandoned' ? TextDecoration.lineThrough : null)),
                  if (task.reason.isNotEmpty) Text(task.reason, style: TextStyle(fontSize: 12, color: muted)),
                ]),
              ),
              Text(_labels[task.status] ?? '待办', style: TextStyle(fontSize: 12, color: muted)),
            ]),
          ),
      ],
    ]);
  }
}
