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
const _pageSize = 30;

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

/// Per-reply trend with a metric switch, 30 replies a page, and the detail table.
class _ReplyTrend extends StatefulWidget {
  const _ReplyTrend({super.key, required this.blocks});
  final List<Block> blocks;
  @override
  State<_ReplyTrend> createState() => _ReplyTrendState();
}

class _ReplyTrendState extends State<_ReplyTrend> {
  String _metric = 'total';
  int _pageOffset = 0;
  bool _details = false;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final small = TextStyle(fontSize: 12, color: scheme.mutedForeground);
    final replies = [
      for (final (index, block) in widget.blocks.whereType<AssistantBlock>().where((block) => !block.live).indexed) (number: index + 1, metrics: block.metrics),
    ];
    final end = (replies.length - _pageOffset * _pageSize).clamp(0, replies.length);
    final page = replies.sublist((end - _pageSize).clamp(0, end), end);
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
          height: 196,
          padding: const EdgeInsets.fromLTRB(4, 12, 16, 4),
          decoration: BoxDecoration(border: Border.all(color: scheme.border), borderRadius: BorderRadius.circular(radius)),
          child: hasData
              ? _TrendChart(points: points, metric: _metric, label: label)
              : Center(child: Text('这组回复没有记录此项数据。', style: small)),
        ),
        if (replies.length > _pageSize)
          Align(
            alignment: Alignment.centerRight,
            child: Wrap(spacing: 4, children: [
              TextButton(onPressed: end <= _pageSize ? null : () => setState(() => _pageOffset++), child: const Text('较早')),
              TextButton(onPressed: _pageOffset == 0 ? null : () => setState(() => _pageOffset--), child: const Text('较新')),
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
