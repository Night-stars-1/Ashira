import 'package:fl_chart/fl_chart.dart';
import 'package:flutter/material.dart';

import '../api/models.dart';
import '../state/ashira_state.dart';
import '../state/display_prefs.dart';
import 'dialogs.dart';
import 'format.dart';
import 'theme.dart';
import 'usage_settings.dart';
import 'widgets.dart';

Future<void> showUsage(BuildContext context, AshiraState state) => showPanel(
      context,
      title: '本会话用量',
      width: 680,
      builder: (_) => ListenableBuilder(listenable: state, builder: (context, _) => _SessionUsage(state)),
    );

class _SessionUsage extends StatelessWidget {
  const _SessionUsage(this.state);
  final AshiraState state;

  @override
  Widget build(BuildContext context) {
    final status = state.status;
    final scheme = Theme.of(context).colorScheme;
    if (status == null) return Center(child: Text('请选择会话。', style: TextStyle(color: scheme.mutedForeground)));
    final stats = status.statistics;
    final usage = stats.usage;
    final cost = stats.cost.main + stats.cost.child;
    // Session average output speed: finished replies' output over their generation time.
    var output = 0.0, ms = 0.0;
    for (final block in state.blocks) {
      if (block is! AssistantBlock || block.live) continue;
      final metrics = block.metrics;
      final usage = metrics?.usage, duration = metrics?.durationMs;
      if (usage == null || duration == null || duration <= 0) continue;
      output += usage.output;
      ms += duration;
    }
    final rate = output > 0 && ms > 0 ? output * 1000 / ms : null;
    return ListView(padding: const EdgeInsets.fromLTRB(20, 16, 20, 20), children: [
      StatCard([
        Stat('Token', wan(usage.prompt + usage.output), tooltip: '${tokens(usage.prompt + usage.output)} tokens（输入 ${tokens(usage.prompt)} · 输出 ${tokens(usage.output)}）'),
        Stat('缓存命中', cacheRate(usage.cacheRead, usage.prompt), tooltip: '输入中 ${tokens(usage.cacheRead)} / ${tokens(usage.prompt)} tokens 来自缓存'),
        Stat('费用', costLabel(cost, stats.cost.mainUnknown || stats.cost.childUnknown), tooltip: stats.cost.mainUnknown || stats.cost.childUnknown ? '部分请求没有单价，未计入' : ''),
        Stat('请求', tokens(stats.mainRequests + stats.childRequests), tooltip: '主代理 ${stats.mainRequests} · 子代理 ${stats.childRequests}'),
        Stat(
          'TPS',
          rate == null ? '—' : rate.toStringAsFixed(1),
          tooltip: rate == null ? '还没有带耗时的回复' : '平均输出速度：${tokens(output.round())} tokens / ${(ms / 1000).toStringAsFixed(1)} 秒',
        ),
      ]),
      const SizedBox(height: 16),
      _ReplyTrend(key: ValueKey(status.sessionId), blocks: state.blocks),
    ]);
  }
}

const _metrics = [('total', '总 Token'), ('output', '输出'), ('hit', '缓存命中'), ('rate', 'TPS'), ('cost', '费用')];
/// Replies the chart shows by default: the latest ones, until the range is moved.
const _defaultSpan = 30;

/// One value per finished reply for a metric; null where the reply has no data.
double? _value(ReplyMetrics? reply, String metric) {
  if (reply == null) return null;
  if (metric == 'rate') return reply.rate;
  if (metric == 'cost') return reply.cost;
  final usage = reply.usage;
  if (usage == null) return null;
  // Share of this reply's prompt read from cache.
  if (metric == 'hit') return usage.prompt == 0 ? null : usage.cacheRead / usage.prompt;
  return (metric == 'output' ? usage.output : usage.prompt + usage.output).toDouble();
}

/// The value a detail column sorts by; null where the reply has none.
double? _cell(({int number, ReplyMetrics? metrics}) reply, String key) {
  final usage = reply.metrics?.usage;
  return switch (key) {
    'number' => reply.number.toDouble(),
    'input' => usage?.prompt.toDouble(),
    'cache' => usage?.cacheRead.toDouble(),
    _ => _value(reply.metrics, key),
  };
}

/// The page in [sort] order; replies without the value stay at the end either way.
List<({int number, ReplyMetrics? metrics})> _sorted(List<({int number, ReplyMetrics? metrics})> page, ReplySort sort) {
  int compare(({int number, ReplyMetrics? metrics}) a, ({int number, ReplyMetrics? metrics}) b) {
    final x = _cell(a, sort.key), y = _cell(b, sort.key);
    if (x == null || y == null) return x == y ? a.number - b.number : (x == null ? 1 : -1);
    final order = sort.ascending ? x.compareTo(y) : y.compareTo(x);
    return order != 0 ? order : a.number - b.number;
  }
  return [...page]..sort(compare);
}

String _format(double? amount, String metric, {bool short = false}) {
  if (amount == null) return '—';
  return switch (metric) {
    'cost' => dollars(amount),
    'hit' => '${(amount * 100).toStringAsFixed(1).replaceFirst(RegExp(r'\.0$'), '')}%',
    'rate' => short ? amount.toStringAsFixed(1) : '${amount.toStringAsFixed(1)} tok/s',
    _ => short ? wan(amount) : tokens(amount),
  };
}

/// Per-reply trend with a metric switch, a range of replies to show, and the detail table.
class _ReplyTrend extends StatefulWidget {
  const _ReplyTrend({super.key, required this.blocks});
  final List<Block> blocks;
  @override
  State<_ReplyTrend> createState() => _ReplyTrendState();
}

class _ReplyTrendState extends State<_ReplyTrend> {
  String _metric = 'total';
  /// The chosen reply numbers, inclusive. Null follows the latest replies as new ones arrive.
  (int, int)? _picked;
  bool _details = false;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final small = TextStyle(fontSize: 12, color: scheme.mutedForeground);
    final replies = [
      for (final (index, block) in widget.blocks.whereType<AssistantBlock>().where((block) => !block.live).indexed) (number: index + 1, metrics: block.metrics),
    ];
    final count = replies.length;
    // `last` keeps the bounds valid while there are no replies yet (nothing is drawn then).
    final last = count < 1 ? 1 : count;
    final (from, to) = switch (_picked) {
      null => ((last - _defaultSpan + 1).clamp(1, last), last),
      (final from, final to) => (from.clamp(1, last), to.clamp(1, last)),
    };
    final page = replies.isEmpty ? replies : replies.sublist(from - 1, to);
    final points = [for (final reply in page) (number: reply.number, value: _value(reply.metrics, _metric))];
    final hasData = points.any((point) => point.value != null);
    final label = _metrics.firstWhere((metric) => metric.$1 == _metric).$2;

    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      Wrap(alignment: WrapAlignment.spaceBetween, crossAxisAlignment: WrapCrossAlignment.center, runSpacing: 8, children: [
        const Text('逐回复趋势', style: TextStyle(fontSize: 14, fontWeight: FontWeight.w600)),
        if (replies.isNotEmpty)
          Wrap(spacing: 4, children: [for (final (id, name) in _metrics) Pill(label: name, selected: _metric == id, onTap: () => setState(() => _metric = id))]),
      ]),
      const SizedBox(height: 8),
      if (replies.isEmpty)
        Text('暂无已完成的回复。', style: small)
      else ...[
        Container(
          padding: const EdgeInsets.fromLTRB(4, 12, 16, 4),
          decoration: BoxDecoration(border: Border.all(color: scheme.border), borderRadius: BorderRadius.circular(radius)),
          child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
            SizedBox(
              height: 180,
              child: hasData
                  ? _TrendChart(points: points, metric: _metric, label: label)
                  : Center(child: Text('这组回复没有记录此项数据。', style: small)),
            ),
            // Every reply in miniature; the box picks which ones the chart above and the table
            // below show.
            if (count > 1) ...[
              const SizedBox(height: 8),
              // The card pads 4 left and 16 right for the chart's axes; 12 more on the left
              // brings the bar to 16 on both sides.
              Padding(
                padding: const EdgeInsets.only(left: 12),
                child: _RangeBrush(
                  values: [for (final reply in replies) _value(reply.metrics, _metric)],
                  from: from,
                  to: to,
                  onChanged: (next) => setState(() {
                    // Moved back onto the newest replies: follow new ones again.
                    _picked = next.$2 >= count && next.$2 - next.$1 + 1 == (count < _defaultSpan ? count : _defaultSpan) ? null : next;
                  }),
                ),
              ),
            ],
          ]),
        ),
        const SizedBox(height: 8),
        Container(
          decoration: BoxDecoration(border: Border.all(color: scheme.border), borderRadius: BorderRadius.circular(radius)),
          child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
            InkWell(
              onTap: () => setState(() => _details = !_details),
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 9),
                child: Row(children: [
                  Icon(_details ? Icons.expand_more : Icons.chevron_right, size: 16),
                  const SizedBox(width: 4),
                  const Text('逐回复明细', style: TextStyle(fontSize: 12, fontWeight: FontWeight.w500)),
                ]),
              ),
            ),
            if (_details)
              ListenableBuilder(
                listenable: displayPrefs,
                builder: (context, _) {
                  final sort = displayPrefs.replySort;
                  return _ReplyTable(
                    sort: sort,
                    // A new column starts with the biggest first; 回复 starts from the first.
                    onSort: (key) => displayPrefs.setReplySort(sort.key == key ? (key: key, ascending: !sort.ascending) : (key: key, ascending: key == 'number')),
                    rows: [
                      for (final reply in _sorted(page, sort))
                        [
                          '#${reply.number}${reply.metrics?.model == null ? '' : '  ${reply.metrics!.model}'}',
                          reply.metrics?.usage == null ? '—' : tokens(reply.metrics!.usage!.prompt),
                          _format(_value(reply.metrics, 'output'), 'output'),
                          reply.metrics?.usage == null ? '—' : '${tokens(reply.metrics!.usage!.cacheRead)} / ${tokens(reply.metrics!.usage!.cacheWrite)}',
                          _format(_value(reply.metrics, 'hit'), 'hit'),
                          _format(_value(reply.metrics, 'rate'), 'rate', short: true),
                          _format(_value(reply.metrics, 'cost'), 'cost'),
                        ],
                    ],
                  );
                },
              ),
          ]),
        ),
      ],
    ]);
  }
}

/// The overview under the trend chart: every reply as a small area, with a box over the
/// replies shown above. Dragging inside the box moves it; dragging near an edge resizes it.
class _RangeBrush extends StatefulWidget {
  const _RangeBrush({required this.values, required this.from, required this.to, required this.onChanged});
  final List<double?> values;
  /// Reply numbers, 1-based and inclusive.
  final int from, to;
  final ValueChanged<(int, int)> onChanged;

  @override
  State<_RangeBrush> createState() => _RangeBrushState();
}

enum _Grip { start, end, move }

class _RangeBrushState extends State<_RangeBrush> {
  static const _bar = 6.0, _track = 24.0, _handleW = 8.0, _handleH = 16.0;
  /// Half a handle of room at each end, so a handle on the first or last reply stays whole.
  static const _pad = _handleW / 2 + 1;
  _Grip? _grip;
  /// Where the drag began, in replies, and the range then; a move keeps its length.
  double _startAt = 0;
  (int, int) _startRange = (1, 1);
  /// The reply numbers beside the handles show only while the pointer is over the bar or a
  /// drag runs, so they never cover the overview at rest.
  bool _hovering = false;

  int get _count => widget.values.length;
  double _xOf(double reply, double width) => _pad + (reply - 1) / (_count - 1) * (width - _pad * 2);
  double _replyAt(double x, double width) => 1 + (x - _pad) / (width - _pad * 2) * (_count - 1);

  void _start(Offset at, double width) {
    final left = _xOf(widget.from.toDouble(), width), right = _xOf(widget.to.toDouble(), width);
    _grip = (at.dx - left).abs() <= _handleW ? _Grip.start
        : (at.dx - right).abs() <= _handleW ? _Grip.end
        : at.dx > left && at.dx < right ? _Grip.move
        : null;
    // A press outside the box centres it there, keeping its length.
    if (_grip == null) {
      final span = widget.to - widget.from;
      final centre = _replyAt(at.dx, width).round();
      final from = (centre - span ~/ 2).clamp(1, _count - span);
      widget.onChanged((from, from + span));
      _grip = _Grip.move;
      _startRange = (from, from + span);
    } else {
      _startRange = (widget.from, widget.to);
    }
    _startAt = _replyAt(at.dx, width);
  }

  void _update(Offset at, double width) {
    final grip = _grip;
    if (grip == null) return;
    final reply = _replyAt(at.dx.clamp(0, width), width).round();
    final (from, to) = _startRange;
    switch (grip) {
      case _Grip.start:
        widget.onChanged((reply.clamp(1, to), to));
      case _Grip.end:
        widget.onChanged((from, reply.clamp(from, _count)));
      case _Grip.move:
        final shift = (_replyAt(at.dx, width) - _startAt).round();
        final next = (from + shift).clamp(1, _count - (to - from));
        widget.onChanged((next, next + to - from));
    }
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return LayoutBuilder(builder: (context, constraints) {
      final width = constraints.maxWidth;
      final cursor = _grip == _Grip.move ? SystemMouseCursors.grabbing : SystemMouseCursors.resizeLeftRight;
      final labels = _hovering || _grip != null;
      return Semantics(
        label: '回复区间 #${widget.from} 到 #${widget.to}',
        child: MouseRegion(
          cursor: cursor,
          onEnter: (_) => setState(() => _hovering = true),
          onExit: (_) => setState(() => _hovering = false),
          child: GestureDetector(
            behavior: HitTestBehavior.opaque,
            onHorizontalDragStart: (details) => setState(() => _start(details.localPosition, width)),
            onHorizontalDragUpdate: (details) => _update(details.localPosition, width),
            onHorizontalDragEnd: (_) => setState(() => _grip = null),
            onTapUp: (details) { _start(details.localPosition, width); setState(() => _grip = null); },
            child: CustomPaint(
              size: Size(width, _bar + _track),
              painter: _BrushPainter(
                values: widget.values,
                from: _xOf(widget.from.toDouble(), width),
                to: _xOf(widget.to.toDouble(), width),
                fromLabel: labels ? '#${widget.from}' : '',
                toLabel: labels ? '#${widget.to}' : '',
                line: inputColor,
                track: scheme.muted,
                handle: scheme.surface,
                text: scheme.mutedForeground,
                bar: _bar,
                handleSize: const Size(_handleW, _handleH),
              ),
            ),
          ),
        ),
      );
    });
  }
}

class _BrushPainter extends CustomPainter {
  _BrushPainter({required this.values, required this.from, required this.to, required this.fromLabel, required this.toLabel, required this.line, required this.track, required this.handle, required this.text, required this.bar, required this.handleSize});
  final List<double?> values;
  final double from, to, bar;
  final String fromLabel, toLabel;
  final Color line, track, handle, text;
  final Size handleSize;

  @override
  void paint(Canvas canvas, Size size) {
    final body = Rect.fromLTRB(0, bar, size.width, size.height);
    canvas.drawRRect(RRect.fromRectAndRadius(body, const Radius.circular(2)), Paint()..color = track.withValues(alpha: .6));
    // Every reply as a faint area, left to right across the same span the box uses.
    final top = values.whereType<double>().fold(0.0, (a, b) => b > a ? b : a);
    final pad = handleSize.width / 2 + 1;
    if (values.length > 1 && top > 0) {
      final path = Path(), area = Path();
      double? first, last;
      for (final (index, value) in values.indexed) {
        if (value == null) continue;
        final point = Offset(pad + index / (values.length - 1) * (size.width - pad * 2), size.height - 2 - value / top * (body.height - 6));
        if (first == null) { path.moveTo(point.dx, point.dy); area..moveTo(point.dx, size.height)..lineTo(point.dx, point.dy); first = point.dx; }
        else { path.lineTo(point.dx, point.dy); area.lineTo(point.dx, point.dy); }
        last = point.dx;
      }
      if (first != null && last != null) {
        area..lineTo(last, size.height)..close();
        canvas.drawPath(area, Paint()..color = line.withValues(alpha: .1));
        canvas.drawPath(path, Paint()..color = line.withValues(alpha: .5)..style = PaintingStyle.stroke..strokeWidth = 1);
      }
    }
    // The box, the bar above it that drags it, and a grip mark in the middle of the bar.
    canvas.drawRect(Rect.fromLTRB(from, bar, to, size.height), Paint()..color = line.withValues(alpha: .15));
    canvas.drawRect(Rect.fromLTRB(from, bar + .5, to, size.height - .5), Paint()..color = line.withValues(alpha: .4)..style = PaintingStyle.stroke..strokeWidth = 1);
    canvas.drawRRect(RRect.fromRectAndRadius(Rect.fromLTRB(from, 0, to, bar), const Radius.circular(2)), Paint()..color = line.withValues(alpha: .45));
    final mid = (from + to) / 2;
    for (final offset in [-3.0, 0.0, 3.0]) {
      canvas.drawRect(Rect.fromLTWH(mid + offset - .5, 1.5, 1, 3), Paint()..color = handle);
    }
    // A small rounded handle on each edge.
    for (final x in [from, to]) {
      final rect = RRect.fromRectAndRadius(Rect.fromCenter(center: Offset(x, bar + body.height / 2), width: handleSize.width, height: handleSize.height), const Radius.circular(3));
      canvas.drawRRect(rect, Paint()..color = handle);
      canvas.drawRRect(rect, Paint()..color = line.withValues(alpha: .7)..style = PaintingStyle.stroke..strokeWidth = 1);
    }
    // Reply numbers beside the handles; inside the box where the edge has no room.
    final gap = handleSize.width / 2 + 4;
    void label(String value, double x, {required bool before}) {
      final painter = TextPainter(text: TextSpan(text: value, style: TextStyle(fontSize: 11, color: text, fontFeatures: const [FontFeature.tabularFigures()])), textDirection: TextDirection.ltr)..layout();
      final outside = before ? x - gap - painter.width >= 0 : x + gap + painter.width <= size.width;
      final left = before == outside ? x - gap - painter.width : x + gap;
      painter.paint(canvas, Offset(left, bar + (body.height - painter.height) / 2));
    }
    label(fromLabel, from, before: true);
    label(toLabel, to, before: false);
  }

  @override
  bool shouldRepaint(_BrushPainter old) => old.values != values || old.from != from || old.to != to || old.fromLabel != fromLabel || old.toLabel != toLabel || old.line != line || old.track != track || old.handle != handle || old.text != text;
}

class _TrendChart extends StatelessWidget {
  const _TrendChart({required this.points, required this.metric, required this.label});
  final List<({int number, double? value})> points;
  final String metric, label;

  static const _leftAxis = 48.0, _bottomAxis = 22.0;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final tick = TextStyle(fontSize: 11, color: scheme.mutedForeground);
    final top = metric == 'hit' ? 1.0 : points.fold<double>(0, (max, point) => (point.value ?? 0) > max ? point.value! : max);
    // The hit rate's axis is 0–100% in halves; others step at a round value up to a tick past the top.
    final yStep = metric == 'hit' ? 0.5 : top == 0 ? 1.0 : axisStep(top);
    final maxY = metric == 'hit' ? 1.0 : top == 0 ? 1.0 : (top / yStep).ceil() * yStep;
    final maxX = points.length == 1 ? 1.0 : (points.length - 1).toDouble();
    // A gap in the data breaks the line instead of drawing through zero.
    final spots = [for (final (index, point) in points.indexed) point.value == null ? FlSpot.nullSpot : FlSpot(index.toDouble(), point.value!)];
    final step = (points.length / 6).ceil().clamp(1, 1 << 30);
    return ChartCrosshair(
      count: points.length,
      leftAxis: _leftAxis,
      bottomAxis: _bottomAxis,
      // Point i sits at i / maxX of the plot width.
      xOf: (index, width) => _leftAxis + (width - _leftAxis) * index / maxX,
      indexAt: (dx, width) => ((dx - _leftAxis) / (width - _leftAxis) * maxX).round(),
      dots: (index) => [(inputColor, points[index].value == null ? null : points[index].value! / maxY)],
      title: (index) => '回复 #${points[index].number}',
      rows: (index) => [(inputColor, label, _format(points[index].value, metric))],
      chart: LineChart(
        LineChartData(
          minY: 0,
          maxY: maxY,
          minX: 0,
          maxX: maxX,
          borderData: FlBorderData(show: false),
          gridData: FlGridData(drawVerticalLine: false, horizontalInterval: yStep, getDrawingHorizontalLine: (_) => FlLine(color: scheme.border, strokeWidth: 1)),
          titlesData: FlTitlesData(
            topTitles: const AxisTitles(),
            rightTitles: const AxisTitles(),
            leftTitles: AxisTitles(
              sideTitles: SideTitles(
                showTitles: true,
                reservedSize: _leftAxis,
                interval: yStep,
                getTitlesWidget: (value, meta) => SideTitleWidget(meta: meta, child: Text(_format(value, metric, short: true), maxLines: 1, softWrap: false, style: tick)),
              ),
            ),
            bottomTitles: AxisTitles(
              sideTitles: SideTitles(
                showTitles: true,
                reservedSize: _bottomAxis,
                interval: 1,
                getTitlesWidget: (value, meta) {
                  final index = value.round();
                  if (value != index || index >= points.length || (index % step != 0 && index != points.length - 1)) return const SizedBox.shrink();
                  return SideTitleWidget(meta: meta, child: Text('#${points[index].number}', style: tick));
                },
              ),
            ),
          ),
          // The crosshair draws the indicator and tooltip, like the web.
          lineTouchData: const LineTouchData(enabled: false),
          lineBarsData: [
            LineChartBarData(
              spots: spots,
              isCurved: true,
              preventCurveOverShooting: true,
              color: inputColor,
              barWidth: 2,
              // A lone reply has no line; the dot keeps it visible.
              dotData: FlDotData(show: points.length == 1),
              belowBarData: BarAreaData(show: true, color: inputColor.withValues(alpha: .15)),
            ),
          ],
        ),
        duration: Duration.zero,
      ),
    );
  }
}

/// The per-reply detail: number columns as wide as their widest cell, the reply and model
/// name in the rest. Narrower than that (phones), every name stays whole and it scrolls sideways.
/// Tapping a header sorts by that column; tapping it again flips the order.
class _ReplyTable extends StatelessWidget {
  const _ReplyTable({required this.rows, required this.sort, required this.onSort});
  final List<List<String>> rows;
  final ReplySort sort;
  final ValueChanged<String> onSort;

  static const _header = ['回复', '输入', '输出', '缓存读 / 写', '缓存命中', 'TPS', '估算费用'];
  /// The arrow beside the sorted column's name, and the gap before it.
  static const _arrow = 14.0;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    const figures = [FontFeature.tabularFigures()];
    final head = TextStyle(fontSize: 12, color: scheme.mutedForeground, fontWeight: FontWeight.w500, fontFeatures: figures);
    final activeHead = head.copyWith(color: scheme.onSurface);
    const body = TextStyle(fontSize: 12, fontFeatures: figures);
    final name = body.copyWith(fontWeight: FontWeight.w500);
    final sorted = replySortKeys.indexOf(sort.key);
    EdgeInsets pad(int column) => EdgeInsets.fromLTRB(column == 0 ? 12 : 16, 7, column == _header.length - 1 ? 12 : 0, 7);
    Widget cell(String text, int column, TextStyle style) => Padding(
          padding: pad(column),
          child: Text(text, maxLines: 1, softWrap: false, overflow: column == 0 ? TextOverflow.ellipsis : TextOverflow.visible, textAlign: column == 0 ? TextAlign.left : TextAlign.right, style: style),
        );
    Widget headCell(String text, int column) {
      final active = column == sorted;
      return Semantics(
        button: true,
        label: '按$text排序',
        child: MouseRegion(
          cursor: SystemMouseCursors.click,
          child: GestureDetector(
            behavior: HitTestBehavior.opaque,
            onTap: () => onSort(replySortKeys[column]),
            child: Padding(
              padding: pad(column),
              child: Row(mainAxisAlignment: column == 0 ? MainAxisAlignment.start : MainAxisAlignment.end, children: [
                Flexible(child: Text(text, maxLines: 1, softWrap: false, overflow: TextOverflow.ellipsis, style: active ? activeHead : head)),
                if (active) ...[
                  const SizedBox(width: 2),
                  Icon(sort.ascending ? Icons.arrow_upward : Icons.arrow_downward, size: 12, color: scheme.onSurface),
                ],
              ]),
            ),
          ),
        ),
      );
    }
    final border = BoxDecoration(border: Border(top: BorderSide(color: scheme.border)));
    Table table(TableColumnWidth first) => Table(
          columnWidths: {0: first},
          defaultColumnWidth: const IntrinsicColumnWidth(),
          defaultVerticalAlignment: TableCellVerticalAlignment.middle,
          children: [
            TableRow(decoration: border, children: [for (final (column, text) in _header.indexed) headCell(text, column)]),
            for (final row in rows) TableRow(decoration: border, children: [for (final (column, text) in row.indexed) cell(text, column, column == 0 ? name : body)]),
          ],
        );
    return LayoutBuilder(builder: (context, constraints) {
      // The widest cell of each column plus its padding: when that fits, the first column takes the slack.
      final scale = MediaQuery.textScalerOf(context);
      final base = DefaultTextStyle.of(context).style;
      double measure(String text, TextStyle style) => (TextPainter(text: TextSpan(text: text, style: base.merge(style)), textDirection: TextDirection.ltr, maxLines: 1, textScaler: scale)..layout()).width;
      var needed = 0.0;
      for (var column = 0; column < _header.length; column++) {
        var widest = measure(_header[column], column == sorted ? activeHead : head) + (column == sorted ? _arrow : 0);
        for (final row in rows) {
          final width = measure(row[column], column == 0 ? name : body);
          if (width > widest) widest = width;
        }
        needed += widest.ceilToDouble() + pad(column).horizontal;
      }
      if (needed <= constraints.maxWidth) return table(const FlexColumnWidth());
      return SingleChildScrollView(scrollDirection: Axis.horizontal, child: table(const IntrinsicColumnWidth()));
    });
  }
}
