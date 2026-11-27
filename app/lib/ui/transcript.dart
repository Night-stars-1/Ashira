import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';

import '../api/models.dart';
import '../state/display_prefs.dart';
import 'image_viewer.dart';
import 'markdown.dart';
import 'text_file_card.dart';
import 'theme.dart';
import 'workspace_image.dart';

/// Scrollable chat log. Follows new content while the user is at the bottom.
class Transcript extends StatefulWidget {
  const Transcript({super.key, required this.blocks, this.activity = '', this.onOpenAgent, this.emptyText, this.start = 0, this.pending = const [], this.loadingEarlier = false, this.onLoadEarlier});
  final List<Block> blocks;
  final String activity;
  final void Function(String id)? onOpenAgent;
  final String? emptyText;
  /// The history index of `blocks[0]`, so row keys stay put when earlier pages load.
  final int start;
  /// Messages sent while the reply runs, shown dimmed until the model reads them.
  final List<String> pending;
  final bool loadingEarlier;
  /// Loads the page before [start] when the reader nears the top; null when there is none.
  final Future<bool> Function()? onLoadEarlier;

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
    // An earlier page went in above: keep the same distance from the bottom, so the rows
    // the reader was looking at stay where they were.
    final fromBottom = _fromBottom;
    if (widget.start < oldWidget.start && fromBottom != null) {
      _fromBottom = null;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted || !_scroll.hasClients) return;
        _pinning = true;
        _scroll.jumpTo((_scroll.position.maxScrollExtent - fromBottom).clamp(0, _scroll.position.maxScrollExtent));
        _pinning = false;
      });
      return;
    }
    if (_follow) _pin();
  }

  /// Distance from the bottom when an earlier page was asked for.
  double? _fromBottom;

  void _maybeLoadEarlier(ScrollMetrics position) {
    final load = widget.onLoadEarlier;
    if (load == null || widget.loadingEarlier || position.pixels > 300) return;
    _fromBottom = position.maxScrollExtent - position.pixels;
    load();
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
    if (user) _maybeLoadEarlier(position);
    return false;
  }

  /// The list's extent or viewport changed (new rows measured, a card animating, the
  /// keyboard opening or closing): stay at the bottom, and have open 「思考过程」 cards
  /// re-place their stuck header. A taller viewport at the bottom clamps the offset
  /// without telling the position's listeners, so the cards would not hear of it.
  bool _onMetrics(ScrollMetricsNotification notification) {
    if (notification.depth != 0) return false;
    if (_follow) _pin();
    // After the frame: the header's new place is only known once this layout is done.
    WidgetsBinding.instance.addPostFrameCallback((_) { if (mounted) _metrics.value++; });
    return false;
  }

  final _metrics = ValueNotifier<int>(0);

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
    _metrics.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final rows = _rows(widget.blocks, widget.start);
    final lead = widget.loadingEarlier ? 1 : 0;
    final count = lead + rows.length + widget.pending.length + (widget.activity.isEmpty ? 0 : 1);
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
              itemBuilder: (context, at) {
                final index = at - lead;
                final child = index < 0
                    ? const _LoadingEarlier()
                    : index < rows.length
                    ? switch (rows[index]) {
                        final _Turn turn => _TurnView(turn, relayout: _metrics, key: ValueKey(turn.key)),
                        final block => _BlockView(block: block as Block, onOpenAgent: widget.onOpenAgent),
                      }
                    : index < rows.length + widget.pending.length
                    ? _PendingMessage(widget.pending[index - rows.length])
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

class _LoadingEarlier extends StatelessWidget {
  const _LoadingEarlier();
  @override
  Widget build(BuildContext context) {
    final muted = Theme.of(context).colorScheme.mutedForeground;
    return Row(mainAxisAlignment: MainAxisAlignment.center, children: [
      SizedBox.square(dimension: 12, child: CircularProgressIndicator(strokeWidth: 1.5, color: muted)),
      const SizedBox(width: 8),
      Text('加载更早的消息…', style: TextStyle(fontSize: 12, color: muted)),
    ]);
  }
}

/// A message sent while the reply runs, dimmed until the model reads it.
class _PendingMessage extends StatelessWidget {
  const _PendingMessage(this.text);
  final String text;
  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Opacity(
      opacity: .6,
      child: Align(
        alignment: Alignment.centerRight,
        child: FractionallySizedBox(
          widthFactor: 0.85,
          alignment: Alignment.centerRight,
          child: Column(crossAxisAlignment: CrossAxisAlignment.end, children: [
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
              decoration: BoxDecoration(color: scheme.primary, borderRadius: BorderRadius.circular(16)),
              child: Text(text, style: TextStyle(fontSize: 14, color: scheme.onPrimary, height: 1.5)),
            ),
            const SizedBox(height: 4),
            Text('等待模型读取', style: TextStyle(fontSize: 12, color: scheme.mutedForeground)),
          ]),
        ),
      ),
    );
  }
}

/// One reply between two user messages: every model request's parts, split into the
/// work (thinking, tools and the text between them) and the answer after the last tool.
class _Turn {
  _Turn(this.key, List<ReplyPart> parts, {required this.live, required this.durationMs}) {
    lastTool = parts.lastIndexWhere((part) => part is ToolPart);
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
  /// The newest tool's index in [work] (-1 when none): the same as in the parts, since
  /// every tool comes before the trailing thinking that joins the work.
  late final int lastTool;
  int get steps => work.where((part) => part is ToolPart || (part is TextPart && part.reasoning)).length;
}

/// Blocks with consecutive assistant blocks (one per model request) merged into turns.
/// `start` is the history index of `blocks[0]`: turn keys stay the same as pages load.
List<Object> _rows(List<Block> blocks, int start) {
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
    key ??= start + index;
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
  const _TurnView(this.turn, {required this.relayout, super.key});
  final _Turn turn;
  /// Fires when the list's metrics change without a scroll; the stuck header follows.
  final Listenable relayout;
  @override
  State<_TurnView> createState() => _TurnViewState();
}

class _TurnViewState extends State<_TurnView> {
  /// The user's choice; `null` follows the reply (open while live).
  bool? _toggled;
  /// How far the open header has moved down its card to stay at the top of the list.
  final _stuck = ValueNotifier<double>(0);
  final _card = GlobalKey();
  final _header = GlobalKey();
  ScrollPosition? _position;

  bool get _open => _toggled ?? widget.turn.live;

  @override
  void initState() {
    super.initState();
    widget.relayout.addListener(_follow);
  }

  @override
  void didUpdateWidget(_TurnView old) {
    super.didUpdateWidget(old);
    if (old.relayout == widget.relayout) return;
    old.relayout.removeListener(_follow);
    widget.relayout.addListener(_follow);
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final position = Scrollable.maybeOf(context)?.position;
    if (position == _position) return;
    _position?.removeListener(_follow);
    _position = position?..addListener(_follow);
  }

  @override
  void dispose() {
    widget.relayout.removeListener(_follow);
    _position?.removeListener(_follow);
    _stuck.dispose();
    super.dispose();
  }

  /// The header stays at the top of the list while the rest of its open card is in view:
  /// moved down by as much as the card has scrolled above the top, never past the card's end.
  /// Worked out from the scroll offset, which is already the new one when the list scrolls;
  /// painted positions would still be the last frame's.
  void _follow() {
    final position = _position;
    final card = _card.currentContext?.findRenderObject() as RenderBox?;
    final header = _header.currentContext?.findRenderObject() as RenderBox?;
    final viewport = card == null ? null : RenderAbstractViewport.maybeOf(card);
    if (!_open || position == null || !position.hasPixels || card == null || header == null || viewport == null || !card.hasSize || !header.hasSize) {
      _stuck.value = 0;
      return;
    }
    // The header's place in the card before it is moved: inside the card's border.
    final inset = MatrixUtils.transformPoint(header.getTransformTo(card), Offset.zero).dy - _stuck.value;
    final top = viewport.getOffsetToReveal(card, 0).offset - position.pixels + inset;
    _stuck.value = (-top).clamp(0, (card.size.height - 2 * inset - header.size.height).clamp(0, double.infinity));
  }

  void _toggle() {
    final stuck = _stuck.value > 0;
    setState(() => _toggled = !_open);
    // Folding from the stuck header would leave the reader far below the reply; bring
    // its header back into view instead.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      _follow();
      final card = _card.currentContext;
      if (stuck && card != null) Scrollable.ensureVisible(card, alignmentPolicy: ScrollPositionAlignmentPolicy.keepVisibleAtStart);
    });
  }

  @override
  Widget build(BuildContext context) {
    final turn = widget.turn;
    final scheme = Theme.of(context).colorScheme;
    final open = _open;
    final muted = TextStyle(fontSize: 12, color: scheme.mutedForeground.withValues(alpha: .75), fontFeatures: const [FontFeature.tabularFigures()]);
    // The card grows and shrinks (streaming, folding) without the list scrolling.
    if (open) WidgetsBinding.instance.addPostFrameCallback((_) { if (mounted) _follow(); });
    final header = ValueListenableBuilder<double>(
      valueListenable: _stuck,
      builder: (context, stuck, child) => Transform.translate(
        offset: Offset(0, stuck),
        // A Material, not a painted box: the tap ripple draws on it, above the work below.
        child: Material(
          type: stuck > 0 ? MaterialType.canvas : MaterialType.transparency,
          color: stuck > 0 ? Theme.of(context).scaffoldBackgroundColor : null,
          shape: stuck > 0 ? Border(bottom: BorderSide(color: scheme.border)) : null,
          child: child,
        ),
      ),
      child: InkWell(
        key: _header,
        onTap: _toggle,
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
    );
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      if (turn.work.isNotEmpty)
        Padding(
          padding: EdgeInsets.only(bottom: turn.answer.isEmpty ? 0 : 8),
          child: Container(
            key: _card,
            decoration: BoxDecoration(border: Border.all(color: scheme.border), borderRadius: BorderRadius.circular(radius)),
            clipBehavior: Clip.antiAlias,
            // Laid out bottom-up so the header, listed last, paints over the work and takes
            // taps first while it is moved down over it.
            child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, verticalDirection: VerticalDirection.up, children: [
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
                                // The running reply's newest tool stays open under 默认折叠工具输出.
                                ToolPart() => ToolCard(part, latest: turn.live && index == turn.lastTool),
                              },
                            ),
                        ]),
                      )
                    : const SizedBox(width: double.infinity),
              ),
              header,
            ]),
          ),
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
      case NoteBlock(:final text, :final error, :final role, :final output) when role == 'shell':
        // A background command: its output, as printed, right on the card (like a tool's).
        final color = error ? scheme.error : scheme.mutedForeground;
        return Container(
          decoration: BoxDecoration(border: Border.all(color: error ? scheme.error.withValues(alpha: .4) : scheme.border), borderRadius: BorderRadius.circular(radius)),
          clipBehavior: Clip.antiAlias,
          child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
              child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
                Padding(padding: const EdgeInsets.only(right: 8, top: 2), child: Icon(Icons.terminal, size: 16, color: color)),
                Expanded(child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                  Text('后台命令', style: TextStyle(fontSize: 12, fontWeight: FontWeight.w500, color: color)),
                  Text(text, style: TextStyle(fontSize: 14, color: color, height: 1.5)),
                ])),
              ]),
            ),
            if (output != null) ...[const Divider(), _RawOutput(output)],
          ]),
        );
      case NoteBlock(:final text, :final error, :final agent):
        final color = error ? scheme.error : scheme.mutedForeground;
        final body = Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
          if (agent != null) Padding(padding: const EdgeInsets.only(right: 8, top: 2), child: Icon(Icons.smart_toy_outlined, size: 16, color: color)),
          Expanded(child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            if (agent != null) Text('子代理', style: TextStyle(fontSize: 12, fontWeight: FontWeight.w500, color: color)),
            Text(text, style: TextStyle(fontSize: 14, color: color, height: 1.5)),
          ])),
          if (agent != null) Text('查看对话', style: TextStyle(fontSize: 14, color: color, decoration: TextDecoration.underline, decorationColor: color)),
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

/// Plain command output: monospace, up to 256px tall, wrapped or scrolling sideways per
/// 设置 → 通用 → 显示 like a tool's output.
class _RawOutput extends StatelessWidget {
  const _RawOutput(this.text);
  final String text;
  @override
  Widget build(BuildContext context) => ListenableBuilder(
        listenable: displayPrefs,
        builder: (context, _) {
          final selectable = SelectableText(text, style: monoStyle(context).copyWith(fontSize: 12, height: 1.45));
          return ConstrainedBox(
            constraints: const BoxConstraints(maxHeight: 256),
            child: SingleChildScrollView(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
              child: displayPrefs.wrapToolOutput ? selectable : SingleChildScrollView(scrollDirection: Axis.horizontal, child: selectable),
            ),
          );
        },
      );
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

/// A transcript picture: a data URL (a message just sent) decodes in memory; a history
/// picture's URL loads over the network, and only once it is shown.
ImageProvider? _picture(String src) {
  if (!src.startsWith('data:')) return src.startsWith('http') ? NetworkImage(src) : null;
  final comma = src.indexOf(',');
  if (comma < 0) return null;
  try {
    return MemoryImage(base64Decode(src.substring(comma + 1)));
  } on FormatException {
    return null;
  }
}

/// An image the user sent; a tap shows it full screen.
class _DataImage extends StatefulWidget {
  const _DataImage(this.src);
  final String src;

  @override
  State<_DataImage> createState() => _DataImageState();
}

class _DataImageState extends State<_DataImage> {
  // Decoded once, not on every rebuild of the transcript.
  late ImageProvider? _image = _picture(widget.src);

  @override
  void didUpdateWidget(_DataImage old) {
    super.didUpdateWidget(old);
    if (old.src != widget.src) _image = _picture(widget.src);
  }

  @override
  Widget build(BuildContext context) {
    final image = _image;
    if (image == null) return const SizedBox.shrink();
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: InkWell(
        borderRadius: BorderRadius.circular(12),
        onTap: () => showImageViewer(context, image),
        child: ClipRRect(
          borderRadius: BorderRadius.circular(12),
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxHeight: 240),
            child: Image(image: image, fit: BoxFit.contain, gaplessPlayback: true, errorBuilder: (_, _, _) => const SizedBox.shrink()),
          ),
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
/// (todo board, `ask_user` answers or raw/numbered output) under a divider. A `write`/`edit`
/// card shows its `+`/`−` line counts instead; tapping the header opens the diff. With
/// 默认折叠工具输出 on, a card shows only its header until tapped, except [latest].
class ToolCard extends StatefulWidget {
  const ToolCard(this.tool, {this.latest = false, super.key});
  final ToolPart tool;
  /// The newest tool of the reply still running, which 默认折叠工具输出 leaves open: a
  /// running tool has no output yet, and the last one's result is what the reader awaits.
  final bool latest;
  @override
  State<ToolCard> createState() => _ToolCardState();
}

class _ToolCardState extends State<ToolCard> {
  bool _diffOpen = false;
  /// This card's own choice once its header is tapped; until then 默认折叠工具输出 decides.
  bool? _opened;
  bool _fold = displayPrefs.foldToolOutput;

  @override
  void initState() {
    super.initState();
    displayPrefs.addListener(_prefsChanged);
  }

  @override
  void dispose() {
    displayPrefs.removeListener(_prefsChanged);
    super.dispose();
  }

  /// Flipping the setting resets cards opened or closed by hand to the new default.
  void _prefsChanged() {
    if (displayPrefs.foldToolOutput == _fold) return;
    setState(() {
      _fold = displayPrefs.foldToolOutput;
      _opened = null;
    });
  }

  @override
  Widget build(BuildContext context) {
    final tool = widget.tool;
    final scheme = Theme.of(context).colorScheme;
    final todo = tool.name == 'todo' ? parseTodo(tool.output) : null;
    final asked = tool.name == 'ask_user' ? parseAsk(tool.output) : null;
    final numbered = todo == null && asked == null && tool.output.isNotEmpty ? numberedLines(tool.output) : null;
    final label = switch (tool.name) { 'todo' => '待办', 'ask_user' => '提问', 'generate_image' => '生成图片', _ => tool.name };
    final images = generatedImages(tool);
    // A binary or unreadable file has no counts and shows the text result instead.
    final change = !tool.error && tool.diff?.added != null ? tool.diff : null;
    final hasBody = images.isNotEmpty || tool.image != null || todo != null || asked != null || (tool.output.isNotEmpty && change == null);
    // The running reply's newest tool, a failed tool and the todo board stay open; a
    // write/edit diff already opens on demand, so it is left alone.
    final foldable = _fold && hasBody && change == null && todo == null && !widget.latest && !tool.error;
    final bodyOpen = !foldable || (_opened ?? false);
    // `max-h-64` scroll area for the todo board and raw output; answers are short and show whole.
    Widget scrolled(Widget child) => ConstrainedBox(
          constraints: const BoxConstraints(maxHeight: 256),
          child: SingleChildScrollView(padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8), child: child),
        );
    // 设置 → 通用 → 显示: unwrapped output keeps each line whole and scrolls sideways.
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
            : tool.output.isEmpty || tool.image != null || change != null
                ? null
                : output();
    final header = Padding(
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
          Text('失败', style: TextStyle(fontSize: 12, color: scheme.error))
        else if (change != null) ...[
          Semantics(
            label: '新增 ${change.added} 行，删除 ${change.removed} 行',
            excludeSemantics: true,
            child: Text.rich(
              TextSpan(children: [
                TextSpan(text: '+${change.added}', style: TextStyle(color: _added(context))),
                TextSpan(text: '  −${change.removed}', style: TextStyle(color: scheme.error)),
              ]),
              style: monoStyle(context).copyWith(fontSize: 12, fontFeatures: const [FontFeature.tabularFigures()]),
            ),
          ),
          AnimatedRotation(
            turns: _diffOpen ? .5 : 0,
            duration: const Duration(milliseconds: 150),
            child: Icon(Icons.expand_more, size: 16, color: scheme.mutedForeground),
          ),
        ] else if (foldable)
          AnimatedRotation(
            turns: bodyOpen ? .5 : 0,
            duration: const Duration(milliseconds: 150),
            child: Icon(Icons.expand_more, size: 16, color: scheme.mutedForeground),
          ),
      ]),
    );
    return Container(
      decoration: BoxDecoration(border: Border.all(color: scheme.border), borderRadius: BorderRadius.circular(radius)),
      clipBehavior: Clip.antiAlias,
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        if (change != null || foldable)
          Material(
            type: MaterialType.transparency,
            child: InkWell(
              onTap: () => setState(() => change != null ? _diffOpen = !_diffOpen : _opened = !bodyOpen),
              child: Semantics(
                button: true,
                expanded: change != null ? _diffOpen : bodyOpen,
                hint: change != null ? (_diffOpen ? '收起改动' : '查看改动') : (bodyOpen ? '收起输出' : '展开输出'),
                child: header,
              ),
            ),
          )
        else
          header,
        if (change != null && _diffOpen) ...[const Divider(), _Diff(change)],
        if (bodyOpen) ...[
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
        ],
      ]),
    );
  }
}

/// The web's `text-emerald-700 dark:text-emerald-400`, for added lines.
Color _added(BuildContext context) =>
    Theme.of(context).brightness == Brightness.dark ? const Color(0xFF34D399) : const Color(0xFF047857);

/// A [FileDiff]'s rows, red/green like `git diff`, up to 384px tall. Replayed `edit` rows
/// count from their snippet, so they have no line-number gutter.
class _Diff extends StatelessWidget {
  const _Diff(this.change);
  final FileDiff change;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final added = _added(context);
    final mono = monoStyle(context).copyWith(fontSize: 12, height: 1.45);
    final muted = mono.copyWith(color: scheme.mutedForeground);
    final numbered = change.rows.any((row) => row.old != null || row.line != null);
    return ListenableBuilder(
      listenable: displayPrefs,
      builder: (context, _) {
        final wrap = displayPrefs.wrapToolOutput;
        Widget line(DiffRow row) {
          final (color, background, sign) = switch (row.kind) {
            'added' => (added, added.withValues(alpha: .1), '+'),
            'removed' => (scheme.error, scheme.error.withValues(alpha: .1), '−'),
            'context' => (null, null, ''),
            _ => (scheme.mutedForeground, null, ''),
          };
          final style = mono.copyWith(color: color);
          final number = row.kind == 'removed' ? row.old : row.line;
          return Container(
            color: background,
            padding: const EdgeInsets.symmetric(horizontal: 12),
            child: Row(crossAxisAlignment: CrossAxisAlignment.start, mainAxisSize: wrap ? MainAxisSize.max : MainAxisSize.min, children: [
              if (numbered)
                SizedBox(width: 32, child: SelectionContainer.disabled(child: Text(number?.toString() ?? '', textAlign: TextAlign.right, style: muted))),
              if (numbered) const SizedBox(width: 8),
              SizedBox(width: 12, child: SelectionContainer.disabled(child: Text(sign, style: style))),
              if (wrap) Expanded(child: _DiffText(row, style: style)) else _DiffText(row, style: style, softWrap: false),
            ]),
          );
        }

        final rows = <Widget>[
          for (final (index, row) in change.rows.indexed)
            if (row.kind != 'hunk')
              line(row)
            // A replayed `edit` has no `@@` text: a gap between its hunks is just a line.
            else if (row.text.isNotEmpty)
              Padding(
                padding: EdgeInsets.fromLTRB(12, index > 0 ? 6 : 2, 12, 2),
                child: Text(row.text, style: muted, softWrap: wrap),
              )
            else if (index > 0)
              Padding(padding: const EdgeInsets.symmetric(vertical: 4), child: Divider(color: scheme.border)),
          if (change.omitted > 0) Padding(padding: const EdgeInsets.fromLTRB(12, 4, 12, 0), child: Text('还有 ${change.omitted} 行未显示', style: muted)),
        ];
        final column = SelectionArea(
          child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, mainAxisSize: MainAxisSize.min, children: rows),
        );
        return ConstrainedBox(
          constraints: const BoxConstraints(maxHeight: 384),
          child: SingleChildScrollView(
            padding: const EdgeInsets.symmetric(vertical: 4),
            child: wrap
                ? column
                : SingleChildScrollView(
                    scrollDirection: Axis.horizontal,
                    child: IntrinsicWidth(child: column),
                  ),
          ),
        );
      },
    );
  }
}

/// A diff row's text with its changed characters on a stronger tint.
class _DiffText extends StatelessWidget {
  const _DiffText(this.row, {required this.style, this.softWrap = true});
  final DiffRow row;
  final TextStyle style;
  final bool softWrap;

  @override
  Widget build(BuildContext context) {
    final text = row.text;
    final onSurface = Theme.of(context).colorScheme.onSurface;
    final mark = TextStyle(backgroundColor: (style.color ?? onSurface).withValues(alpha: .3), color: onSurface);
    final spans = <TextSpan>[];
    var at = 0;
    for (final (start, end) in row.marks) {
      if (start < at) continue;
      if (start > at) spans.add(TextSpan(text: text.substring(at, start)));
      spans.add(TextSpan(text: text.substring(start, end), style: mark));
      at = end;
    }
    if (at < text.length) spans.add(TextSpan(text: text.substring(at)));
    return Text.rich(TextSpan(children: spans), style: style, softWrap: softWrap);
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
  late ImageProvider? _image = _picture(widget.src);

  @override
  void didUpdateWidget(_ReadImage old) {
    super.didUpdateWidget(old);
    if (old.src != widget.src) _image = _picture(widget.src);
  }

  @override
  Widget build(BuildContext context) {
    final image = _image;
    if (image == null) return const SizedBox.shrink();
    final muted = Theme.of(context).colorScheme.mutedForeground;
    return Align(
      alignment: Alignment.centerLeft,
      child: Tooltip(
        message: widget.label,
        child: InkWell(
          borderRadius: BorderRadius.circular(radius),
          onTap: () => showImageViewer(context, image, label: widget.label),
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxHeight: 256),
            child: ClipRRect(
              borderRadius: BorderRadius.circular(radius),
              child: Image(
                image: image,
                fit: BoxFit.contain,
                gaplessPlayback: true,
                errorBuilder: (_, _, _) => Text('图片无法加载', style: TextStyle(fontSize: 12, color: muted)),
                loadingBuilder: (context, child, progress) => progress == null
                    ? child
                    : SizedBox.square(dimension: 64, child: Center(child: SizedBox.square(dimension: 16, child: CircularProgressIndicator(strokeWidth: 1.5, color: muted)))),
              ),
            ),
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
