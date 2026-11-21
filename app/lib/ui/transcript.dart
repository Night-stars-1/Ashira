import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter/material.dart';

import '../api/models.dart';
import '../state/display_prefs.dart';
import 'markdown.dart';
import 'text_file_card.dart';
import 'theme.dart';
import 'workspace_image.dart';

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
  /// Set while this widget moves the view, so those scrolls never count as the reader's.
  bool _pinning = false;

  @override
  void initState() {
    super.initState();
    _pin();
  }

  @override
  void didUpdateWidget(Transcript oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (_follow) _pin();
  }

  /// Only a drag, wheel or scrollbar move by the reader decides following: content growing
  /// below (streaming, `AnimatedSize`, a lazily measured long history) moves the bottom
  /// away without them scrolling, and is followed instead.
  bool _onScroll(ScrollNotification notification) {
    if (_pinning || notification.depth != 0) return false;
    final position = notification.metrics;
    final atBottom = position.pixels >= position.maxScrollExtent - 2;
    final user = notification is ScrollUpdateNotification && notification.dragDetails != null
        || notification is UserScrollNotification;
    if (atBottom && !_follow) setState(() => _follow = true);
    if (!atBottom && _follow && user) setState(() => _follow = false);
    return false;
  }

  /// The list's extent changed (new rows measured, a card animating): stay at the bottom.
  bool _onMetrics(ScrollMetricsNotification notification) {
    if (_follow && notification.depth == 0) _pin();
    return false;
  }

  void _pin() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted || !_follow || !_scroll.hasClients) return;
      final position = _scroll.position;
      if (position.pixels == position.maxScrollExtent) return;
      _pinning = true;
      _scroll.jumpTo(position.maxScrollExtent);
      _pinning = false;
    });
  }

  void _toBottom() {
    setState(() => _follow = true);
    _pin();
  }

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final rows = _rows(widget.blocks);
    final count = rows.length + (widget.activity.isEmpty ? 0 : 1);
    if (count == 0) {
      return Center(child: Text(widget.emptyText ?? '没有对话', style: TextStyle(color: Theme.of(context).colorScheme.onSurfaceVariant)));
    }
    final wide = MediaQuery.sizeOf(context).width >= 640;
    final scheme = Theme.of(context).colorScheme;
    return Stack(children: [
      NotificationListener<ScrollMetricsNotification>(
        onNotification: _onMetrics,
        child: NotificationListener<ScrollNotification>(
          onNotification: _onScroll,
          child: Scrollbar(
            controller: _scroll,
            child: ListView.builder(
              controller: _scroll,
              padding: EdgeInsets.symmetric(horizontal: wide ? 24 : 12, vertical: wide ? 24 : 16),
              itemCount: count,
              itemBuilder: (context, index) {
                final child = index < rows.length
                    ? switch (rows[index]) {
                        final _Turn turn => _TurnView(turn, key: ValueKey(turn.key)),
                        final block => _BlockView(block: block as Block, onOpenAgent: widget.onOpenAgent),
                      }
                    : _Activity(widget.activity);
                return Center(
                  child: ConstrainedBox(constraints: const BoxConstraints(maxWidth: 768), child: Padding(padding: const EdgeInsets.only(bottom: 16), child: child)),
                );
              },
            ),
          ),
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

/// One reply between two user messages: every model request's parts, split into the
/// work (thinking, tools and the text between them) and the answer after the last tool.
class _Turn {
  _Turn(this.key, List<ReplyPart> parts, {required this.live, required this.durationMs}) {
    final lastTool = parts.lastIndexWhere((part) => part is ToolPart);
    final tail = parts.sublist(lastTool + 1);
    // Thinking after the last tool still belongs to the work; only its text is the answer.
    work = [...parts.sublist(0, lastTool + 1), ...tail.where((part) => part is TextPart && part.reasoning)];
    answer = [for (final part in tail) if (part is TextPart && !part.reasoning && part.text.isNotEmpty) part.text];
    images = [for (final part in parts) if (part is ToolPart) ...generatedImages(part)];
  }
  final int key;
  final bool live;
  final double? durationMs;
  late final List<ReplyPart> work;
  late final List<String> answer;
  /// Pictures `generate_image` saved: results, shown with the answer outside the fold.
  late final List<String> images;
  int get steps => work.where((part) => part is ToolPart || (part is TextPart && part.reasoning)).length;
}

/// Blocks with consecutive assistant blocks (one per model request) merged into turns.
List<Object> _rows(List<Block> blocks) {
  final rows = <Object>[];
  int? key;
  var parts = <ReplyPart>[];
  var live = false;
  double? duration;
  void flush() {
    if (key != null) rows.add(_Turn(key!, parts, live: live, durationMs: duration));
    key = null;
    parts = [];
    live = false;
    duration = null;
  }

  for (final (index, block) in blocks.indexed) {
    if (block is! AssistantBlock) {
      flush();
      rows.add(block);
      continue;
    }
    key ??= index;
    parts.addAll(block.parts);
    live = live || block.live;
    final ms = block.metrics?.durationMs;
    if (ms != null) duration = (duration ?? 0) + ms;
  }
  flush();
  return rows;
}

String _duration(double ms) {
  final seconds = (ms / 1000).round();
  return seconds >= 60 ? '${seconds ~/ 60}分${seconds % 60}秒' : '${seconds < 1 ? 1 : seconds}秒';
}

/// The web's reply: a 「思考过程」 card, open while the reply runs and folded once it has
/// finished (a tap sticks), then the answer itself.
class _TurnView extends StatefulWidget {
  const _TurnView(this.turn, {super.key});
  final _Turn turn;
  @override
  State<_TurnView> createState() => _TurnViewState();
}

class _TurnViewState extends State<_TurnView> {
  /// The user's choice; `null` follows the reply (open while live).
  bool? _toggled;

  @override
  Widget build(BuildContext context) {
    final turn = widget.turn;
    final scheme = Theme.of(context).colorScheme;
    final open = _toggled ?? turn.live;
    final muted = TextStyle(fontSize: 12, color: scheme.mutedForeground.withValues(alpha: .75), fontFeatures: const [FontFeature.tabularFigures()]);
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      if (turn.work.isNotEmpty)
        Container(
          margin: EdgeInsets.only(bottom: turn.answer.isEmpty ? 0 : 8),
          decoration: BoxDecoration(border: Border.all(color: scheme.border), borderRadius: BorderRadius.circular(radius)),
          clipBehavior: Clip.antiAlias,
          child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
            InkWell(
              onTap: () => setState(() => _toggled = !open),
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                child: Row(children: [
                  AnimatedRotation(turns: open ? .25 : 0, duration: const Duration(milliseconds: 150), child: Icon(Icons.chevron_right, size: 14, color: scheme.mutedForeground)),
                  const SizedBox(width: 6),
                  if (turn.live) ...[
                    SizedBox.square(dimension: 12, child: CircularProgressIndicator(strokeWidth: 1.5, color: scheme.mutedForeground)),
                    const SizedBox(width: 6),
                  ],
                  Text(turn.live ? '正在思考' : '思考过程', style: TextStyle(fontSize: 14, fontWeight: FontWeight.w500, color: scheme.mutedForeground)),
                  const SizedBox(width: 6),
                  Text('${turn.steps} 步${turn.durationMs != null && turn.durationMs! > 0 ? ' · 用时 ${_duration(turn.durationMs!)}' : ''}', style: muted),
                ]),
              ),
            ),
            AnimatedSize(
              duration: const Duration(milliseconds: 150),
              alignment: Alignment.topCenter,
              child: open
                  ? Container(
                      decoration: BoxDecoration(border: Border(top: BorderSide(color: scheme.border))),
                      padding: const EdgeInsets.fromLTRB(12, 8, 12, 0),
                      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                        for (final (index, part) in turn.work.indexed)
                          Padding(
                            padding: const EdgeInsets.only(bottom: 8),
                            child: switch (part) {
                              // The last part of a live reply with no answer yet is still being written.
                              TextPart(reasoning: true, :final text) => _Reasoning(text, streaming: turn.live && turn.answer.isEmpty && index == turn.work.length - 1),
                              TextPart(:final text) => Markdown(text),
                              ToolPart() => ToolCard(part),
                            },
                          ),
                      ]),
                    )
                  : const SizedBox(width: double.infinity),
            ),
          ]),
        ),
      if (turn.images.isNotEmpty)
        Padding(
          padding: EdgeInsets.only(bottom: turn.answer.isEmpty ? 0 : 8),
          child: Wrap(spacing: 8, runSpacing: 8, children: [for (final path in turn.images) WorkspaceImage(path)]),
        ),
      for (final (index, text) in turn.answer.indexed) Padding(padding: EdgeInsets.only(top: index == 0 ? 0 : 8), child: Markdown(text)),
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
      case AssistantBlock():
        throw StateError('assistant blocks render as turns');
    }
  }
}

/// The web's reasoning block: short thoughts show inline; long ones (over 6 lines
/// or 400 characters) show in full while streaming, then fold to a one-line preview.
class _Reasoning extends StatefulWidget {
  const _Reasoning(this.text, {this.streaming = false});
  final String text;
  final bool streaming;
  @override
  State<_Reasoning> createState() => _ReasoningState();
}

class _ReasoningState extends State<_Reasoning> {
  /// Flips the default (open while streaming, folded after).
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
    final open = widget.streaming != _toggled;
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

/// Workspace paths a successful `generate_image` call saved (one `saved: <path>` line each).
List<String> generatedImages(ToolPart tool) => tool.name == 'generate_image' && !tool.error
    ? [for (final line in tool.output.split('\n')) if (line.startsWith('saved: ')) line.substring(7).trim()]
    : const [];

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
    final label = switch (tool.name) { 'todo' => '待办', 'ask_user' => '提问', 'generate_image' => '生成图片', _ => tool.name };
    final images = generatedImages(tool);
    // `max-h-64` scroll area for the todo board and raw output; answers are short and show whole.
    Widget scrolled(Widget child) => ConstrainedBox(
          constraints: const BoxConstraints(maxHeight: 256),
          child: SingleChildScrollView(padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8), child: child),
        );
    // 设置 → 显示: unwrapped output keeps each line whole and scrolls sideways.
    Widget output() => ListenableBuilder(
          listenable: displayPrefs,
          builder: (context, _) {
            final wrap = displayPrefs.wrapToolOutput;
            final text = numbered != null
                ? _Numbered(numbered, wrap: wrap)
                : SelectableText(tool.output, style: monoStyle(context).copyWith(fontSize: 12, height: 1.45));
            return scrolled(wrap ? text : SingleChildScrollView(scrollDirection: Axis.horizontal, child: text));
          },
        );
    final Widget? body = todo != null
        ? scrolled(_TodoBoard(todo))
        : asked != null
            ? Padding(padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8), child: _AskResult(asked))
            // A picture `read` returned is the result; its "attached for you to view" note is for the model.
            : tool.output.isEmpty || tool.image != null
                ? null
                : output();
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
        if (images.isNotEmpty) ...[
          const Divider(),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
            child: Wrap(spacing: 8, runSpacing: 8, children: [for (final path in images) WorkspaceImage(path, maxHeight: 256)]),
          ),
        ],
        if (tool.image case final image?) ...[
          const Divider(),
          Padding(padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8), child: _ReadImage(image, label: tool.summary)),
        ],
        if (body != null) ...[const Divider(), body],
      ]),
    );
  }
}

/// The picture `read` gave the model (a data URL), decoded once; tap shows it full size.
class _ReadImage extends StatefulWidget {
  const _ReadImage(this.src, {required this.label});
  final String src, label;
  @override
  State<_ReadImage> createState() => _ReadImageState();
}

class _ReadImageState extends State<_ReadImage> {
  late Uint8List? _bytes = _decode(widget.src);

  static Uint8List? _decode(String src) {
    final comma = src.indexOf(',');
    if (comma < 0) return null;
    try {
      return base64Decode(src.substring(comma + 1));
    } on FormatException {
      return null;
    }
  }

  @override
  void didUpdateWidget(_ReadImage old) {
    super.didUpdateWidget(old);
    if (old.src != widget.src) _bytes = _decode(widget.src);
  }

  @override
  Widget build(BuildContext context) {
    final bytes = _bytes;
    if (bytes == null) return const SizedBox.shrink();
    return Align(
      alignment: Alignment.centerLeft,
      child: Tooltip(
        message: widget.label,
        child: InkWell(
          borderRadius: BorderRadius.circular(radius),
          onTap: () => showDialog<void>(
            context: context,
            builder: (context) => Dialog(
              insetPadding: const EdgeInsets.all(24),
              clipBehavior: Clip.antiAlias,
              child: InteractiveViewer(maxScale: 6, child: Image.memory(bytes, fit: BoxFit.contain, gaplessPlayback: true)),
            ),
          ),
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxHeight: 256),
            child: ClipRRect(borderRadius: BorderRadius.circular(radius), child: Image.memory(bytes, fit: BoxFit.contain, gaplessPlayback: true)),
          ),
        ),
      ),
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
  const _Numbered(this.rows, {required this.wrap});
  final List<({String n, String text})> rows;
  /// Off: each line keeps its natural width inside a sideways scroll view.
  final bool wrap;
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
            if (wrap) Expanded(child: Text(row.text, style: mono)) else Text(row.text, style: mono, softWrap: false),
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
