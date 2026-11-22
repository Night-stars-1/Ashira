import 'dart:math' as math;

import 'package:fl_chart/fl_chart.dart';
import 'package:flutter/material.dart';

import '../api/client.dart';
import '../api/models.dart';
import 'dialogs.dart';
import 'format.dart';
import 'theme.dart';
import 'widgets.dart';

const _ranges = [('24h', '24 小时'), ('today', '今天'), ('7d', '7 天'), ('30d', '30 天'), ('all', '全部')];
const inputColor = Color(0xFF0EA5E9);
const outputColor = Color(0xFFF59E0B);

/// Sums of usage buckets; `prompt` is every input token (uncached + cache read + cache write).
class UsageSums {
  int input = 0, output = 0, cacheRead = 0, cacheWrite = 0, unknown = 0, requests = 0, timedOutput = 0, durationMs = 0;
  double cost = 0;
  int get prompt => input + cacheRead + cacheWrite;
  int get tokens => prompt + output;

  void add(UsageBucket bucket) {
    input += bucket.usage.input;
    output += bucket.usage.output;
    cacheRead += bucket.usage.cacheRead;
    cacheWrite += bucket.usage.cacheWrite;
    cost += bucket.cost;
    unknown += bucket.unknown;
    requests += bucket.requests;
    timedOutput += bucket.timedOutput;
    durationMs += bucket.durationMs;
  }
}

/// Share of prompt tokens served from cache.
String cacheRate(int cacheRead, int prompt) => prompt == 0 ? '—' : '${_trim(cacheRead / prompt * 100)}%';

/// A cost with unpriced requests is a lower bound, or unknown when nothing was priced.
String costLabel(double cost, bool unknown) => !unknown ? dollars(cost) : cost > 0 ? '≥ ${dollars(cost)}' : '未知';

/// Average output speed over the requests that logged their generation time.
String tpsLabel(UsageSums sums) => sums.durationMs == 0 ? '—' : (sums.timedOutput * 1000 / sums.durationMs).toStringAsFixed(1);

String _trim(double value) => value.toStringAsFixed(1).replaceFirst(RegExp(r'\.0$'), '');
String _pad(int value) => value.toString().padLeft(2, '0');
String _day(DateTime time) => '${time.year}-${_pad(time.month)}-${_pad(time.day)}';
DateTime _hourTime(int hour) => DateTime.fromMillisecondsSinceEpoch(hour * 3600000);

/// One headline number with a label, as in the web summary cards.
class Stat extends StatelessWidget {
  const Stat(this.label, this.value, {super.key, this.tooltip = ''});
  final String label, value, tooltip;
  @override
  Widget build(BuildContext context) {
    final body = Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      Text(label, style: TextStyle(fontSize: 12, color: Theme.of(context).colorScheme.mutedForeground)),
      const SizedBox(height: 4),
      Text(value, maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 18, fontWeight: FontWeight.w600, fontFeatures: [FontFeature.tabularFigures()])),
    ]);
    return tooltip.isEmpty ? body : Tooltip(message: tooltip, child: body);
  }
}

/// A tinted card of stats that wraps two per row on narrow screens.
class StatCard extends StatelessWidget {
  const StatCard(this.stats, {super.key});
  final List<Stat> stats;
  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Container(
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(color: scheme.muted.withValues(alpha: .3), border: Border.all(color: scheme.border), borderRadius: BorderRadius.circular(radius)),
      child: LayoutBuilder(builder: (context, constraints) {
        final columns = constraints.maxWidth < 420 ? 2 : stats.length;
        final width = (constraints.maxWidth - (columns - 1) * 12) / columns;
        return Wrap(spacing: 12, runSpacing: 12, children: [for (final stat in stats) SizedBox(width: width, child: stat)]);
      }),
    );
  }
}

/// Settings → 用量: every session (archived and subagents included), by time and model.
class UsageSettings extends StatefulWidget {
  const UsageSettings({super.key, required this.client});
  final AshiraClient client;
  @override
  State<UsageSettings> createState() => _UsageSettingsState();
}

class _UsageSettingsState extends State<UsageSettings> {
  UsageReport? _report;
  String _error = '';
  String _range = '30d';

  bool get _hourly => _range == '24h' || _range == 'today';

  @override
  void initState() {
    super.initState();
    widget.client.usage().then((report) {
      if (mounted) setState(() => _report = report);
    }).catchError((Object error) {
      if (mounted) setState(() => _error = errorText(error, '读取用量失败'));
    });
  }

  int get _nowHour => DateTime.now().millisecondsSinceEpoch ~/ 3600000;

  /// First unix hour inside the range (fractional at local midnight).
  double get _since {
    if (_range == 'all') return 0;
    if (_range == '24h') return (_nowHour - 23).toDouble();
    final now = DateTime.now();
    final back = {'today': 0, '7d': 6, '30d': 29}[_range]!;
    return DateTime(now.year, now.month, now.day - back).millisecondsSinceEpoch / 3600000;
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final report = _report;
    if (_error.isNotEmpty) return Text(_error, style: TextStyle(fontSize: 13, color: scheme.error));
    if (report == null) return const Center(child: Padding(padding: EdgeInsets.all(24), child: SizedBox.square(dimension: 18, child: CircularProgressIndicator())));
    final since = _since;
    final rows = report.buckets.where((bucket) => bucket.hour + 1 > since).toList();
    final small = TextStyle(fontSize: 12, color: scheme.mutedForeground);

    final all = UsageSums();
    final byModel = <String, UsageSums>{};
    for (final bucket in rows) {
      all.add(bucket);
      byModel.putIfAbsent(bucket.model, UsageSums.new).add(bucket);
    }
    final models = byModel.entries.toList()..sort((left, right) => right.value.tokens.compareTo(left.value.tokens));
    final stats = [
      Stat('Token', wan(all.tokens), tooltip: '${tokens(all.tokens)} tokens（输入 ${tokens(all.prompt)} · 输出 ${tokens(all.output)}）'),
      Stat('缓存命中', cacheRate(all.cacheRead, all.prompt), tooltip: '输入中 ${tokens(all.cacheRead)} / ${tokens(all.prompt)} tokens 来自缓存'),
      Stat('费用', costLabel(all.cost, all.unknown > 0), tooltip: all.unknown > 0 ? '${all.unknown} 个请求没有单价，未计入' : ''),
      Stat('请求', tokens(all.requests)),
      if (!_hourly) Stat('活跃天数', '${rows.map((bucket) => _day(_hourTime(bucket.hour))).toSet().length}'),
    ];

    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      Wrap(alignment: WrapAlignment.spaceBetween, crossAxisAlignment: WrapCrossAlignment.center, runSpacing: 8, children: [
        Text('所有会话（含已归档、子代理）· 共 ${report.sessions} 个会话', style: small),
        Wrap(spacing: 4, children: [for (final (id, label) in _ranges) Pill(label: label, selected: _range == id, onTap: () => setState(() => _range = id))]),
      ]),
      const SizedBox(height: 12),
      if (rows.isEmpty)
        const EmptyState(title: '这段时间没有用量', description: '发起对话后，这里会按时间和模型汇总所有会话的 Token 与费用。')
      else ...[
        StatCard(stats),
        const SizedBox(height: 12),
        Container(
          padding: const EdgeInsets.all(12),
          decoration: BoxDecoration(border: Border.all(color: scheme.border), borderRadius: BorderRadius.circular(radius)),
          child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
            Text(_hourly ? '每小时 Token' : '每日 Token', style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w500)),
            const SizedBox(height: 8),
            SizedBox(height: 176, child: _TokenBars(slots: _slots(rows, since))),
          ]),
        ),
        const SizedBox(height: 12),
        Container(
          decoration: BoxDecoration(border: Border.all(color: scheme.border), borderRadius: BorderRadius.circular(radius)),
          child: Column(children: [
            _ModelRow(cells: const ['模型', 'Token', '缓存命中', '请求', 'TPS', '费用'], header: true),
            for (final MapEntry(key: model, value: sums) in models)
              _ModelRow(cells: [
                model,
                wan(sums.tokens),
                cacheRate(sums.cacheRead, sums.prompt),
                tokens(sums.requests),
                tpsLabel(sums),
                costLabel(sums.cost, sums.unknown > 0),
              ], tooltip: '输入 ${tokens(sums.prompt)} · 输出 ${tokens(sums.output)}'),
          ]),
        ),
      ],
    ]);
  }

  /// One bar per hour or local day; empty slots stay so gaps read as gaps.
  List<_Slot> _slots(List<UsageBucket> rows, double since) {
    String keyOf(int hour) => _hourly ? '$hour' : _day(_hourTime(hour));
    final sums = <String, ({int input, int output})>{};
    for (final bucket in rows) {
      final key = keyOf(bucket.hour);
      final current = sums[key] ?? (input: 0, output: 0);
      sums[key] = (input: current.input + bucket.usage.input + bucket.usage.cacheRead + bucket.usage.cacheWrite, output: current.output + bucket.usage.output);
    }
    if (sums.isEmpty) return const [];
    final slots = <_Slot>[];
    void push(String key, String tick, String title) => slots.add(_Slot(tick, title, sums[key]?.input ?? 0, sums[key]?.output ?? 0));
    if (_hourly) {
      for (var hour = since.ceil(); hour <= _nowHour; hour++) {
        final clock = '${_pad(_hourTime(hour).hour)}:00';
        push('$hour', clock, '${_day(_hourTime(hour))} $clock');
      }
    } else {
      final first = _range == 'all' ? DateTime.parse('${(sums.keys.toList()..sort()).first}T00:00:00') : DateTime.fromMillisecondsSinceEpoch((since * 3600000).round());
      final today = DateTime.now();
      for (var date = DateTime(first.year, first.month, first.day); !date.isAfter(today); date = DateTime(date.year, date.month, date.day + 1)) {
        final day = _day(date);
        push(day, '${_pad(date.month)}/${_pad(date.day)}', day);
      }
    }
    return slots;
  }
}

class _Slot {
  const _Slot(this.tick, this.title, this.input, this.output);
  final String tick, title;
  final int input, output;
}

/// Stacked input/output bars, like the web's VisStackedBar, with the web's crosshair.
class _TokenBars extends StatelessWidget {
  const _TokenBars({required this.slots});
  final List<_Slot> slots;

  static const _leftAxis = 48.0, _bottomAxis = 22.0;

  double get _barWidth => (360 / slots.length).clamp(3, 18).toDouble();

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final top = slots.fold<int>(0, (max, slot) => slot.input + slot.output > max ? slot.input + slot.output : max);
    final step = top == 0 ? 1.0 : axisStep(top.toDouble());
    final maxY = top == 0 ? 1.0 : (top / step).ceil() * step;
    final every = (slots.length / 7).ceil().clamp(1, 1 << 30);
    final label = TextStyle(fontSize: 11, color: scheme.mutedForeground);
    // `spaceBetween` puts the first and last bar edges on the plot edges, so bar
    // centers run from half a bar in on the left to half a bar in on the right.
    double span(double width) => width - _leftAxis - _barWidth;
    return ChartCrosshair(
      count: slots.length,
      leftAxis: _leftAxis,
      bottomAxis: _bottomAxis,
      xOf: (index, width) => _leftAxis + _barWidth / 2 + (slots.length == 1 ? span(width) / 2 : span(width) * index / (slots.length - 1)),
      indexAt: (dx, width) => slots.length == 1 ? 0 : (((dx - _leftAxis - _barWidth / 2) / span(width)) * (slots.length - 1)).round(),
      // One dot per series at the top of its stacked segment.
      dots: (index) => [(inputColor, slots[index].input / maxY), (outputColor, (slots[index].input + slots[index].output) / maxY)],
      title: (index) => slots[index].title,
      rows: (index) => [(inputColor, '输入', wan(slots[index].input)), (outputColor, '输出', wan(slots[index].output))],
      chart: BarChart(
        BarChartData(
          maxY: maxY,
          alignment: BarChartAlignment.spaceBetween,
          borderData: FlBorderData(show: false),
          gridData: FlGridData(drawVerticalLine: false, horizontalInterval: step, getDrawingHorizontalLine: (_) => FlLine(color: scheme.border, strokeWidth: 1)),
          titlesData: FlTitlesData(
            topTitles: const AxisTitles(),
            rightTitles: const AxisTitles(),
            leftTitles: AxisTitles(
              sideTitles: SideTitles(
                showTitles: true,
                reservedSize: _leftAxis,
                interval: step,
                getTitlesWidget: (value, meta) => SideTitleWidget(meta: meta, child: Text(wan(value.round()), maxLines: 1, softWrap: false, style: label)),
              ),
            ),
            bottomTitles: AxisTitles(
              sideTitles: SideTitles(
                showTitles: true,
                reservedSize: _bottomAxis,
                getTitlesWidget: (value, meta) {
                  final index = value.toInt();
                  if (index % every != 0 && index != slots.length - 1) return const SizedBox.shrink();
                  return SideTitleWidget(meta: meta, child: Text(slots[index].tick, style: label));
                },
              ),
            ),
          ),
          // The crosshair handles hover, so empty slots get the indicator too.
          barTouchData: BarTouchData(enabled: false),
          barGroups: [
            for (final (index, slot) in slots.indexed)
              BarChartGroupData(x: index, barRods: [
                BarChartRodData(
                  toY: (slot.input + slot.output).toDouble(),
                  width: _barWidth,
                  borderRadius: const BorderRadius.vertical(top: Radius.circular(2)),
                  color: Colors.transparent,
                  rodStackItems: [
                    BarChartRodStackItem(0, slot.input.toDouble(), inputColor),
                    BarChartRodStackItem(slot.input.toDouble(), (slot.input + slot.output).toDouble(), outputColor),
                  ],
                ),
              ]),
          ],
        ),
        duration: Duration.zero,
      ),
    );
  }
}

/// A round 1/2/5 × 10ⁿ y-axis step giving about two ticks above zero, like the web's
/// `numTicks: 3`. Topping the axis out on a tick keeps the labels short (`500万`, not `1021.6万`).
double axisStep(double top) {
  final rough = top / 2;
  final power = math.pow(10, (math.log(rough) / math.ln10).floor()).toDouble();
  final unit = rough / power;
  return (unit <= 1 ? 1 : unit <= 2 ? 2 : unit <= 5 ? 5 : 10) * power;
}

/// The web's unovis crosshair over an fl_chart: anywhere in a column shows a 1px #888
/// line across the plot, a dot per series at its value, and the ChartTooltipContent
/// card beside the pointer. Line and dots slide 100ms between columns, like unovis.
class ChartCrosshair extends StatefulWidget {
  const ChartCrosshair({
    super.key,
    required this.chart,
    required this.count,
    required this.leftAxis,
    required this.bottomAxis,
    required this.xOf,
    required this.indexAt,
    required this.dots,
    required this.title,
    required this.rows,
  });
  final Widget chart;
  final int count;

  /// Reserved axis sizes; the plot spans the rest.
  final double leftAxis, bottomAxis;

  /// Column center in the chart box, and the column nearest a pointer x.
  final double Function(int index, double width) xOf;
  final int Function(double dx, double width) indexAt;

  /// Dots as (color, value / maxY); a null fraction draws no dot.
  final List<(Color, double?)> Function(int index) dots;
  final String Function(int index) title;
  final List<(Color, String, String)> Function(int index) rows;

  @override
  State<ChartCrosshair> createState() => _ChartCrosshairState();
}

class _ChartCrosshairState extends State<ChartCrosshair> {
  int? _hover;
  Offset _pointer = Offset.zero;

  void _track(Offset position, Size size) {
    final inside = widget.count > 0 && position.dx >= widget.leftAxis && position.dy <= size.height - widget.bottomAxis;
    final index = inside ? widget.indexAt(position.dx, size.width).clamp(0, widget.count - 1) : null;
    if (index == _hover && position == _pointer) return;
    setState(() {
      _hover = index;
      _pointer = position;
    });
  }

  void _leave() {
    if (_hover != null) setState(() => _hover = null);
  }

  @override
  Widget build(BuildContext context) => LayoutBuilder(builder: (context, constraints) {
        final size = constraints.biggest;
        final hover = _hover != null && _hover! < widget.count ? _hover : null;
        final plotBottom = size.height - widget.bottomAxis;
        final x = hover == null ? 0.0 : widget.xOf(hover, size.width);
        const move = Duration(milliseconds: 100);
        return MouseRegion(
          onHover: (event) => _track(event.localPosition, size),
          onExit: (_) => _leave(),
          child: GestureDetector(
            // Phones: touch and drag move the indicator.
            onPanDown: (details) => _track(details.localPosition, size),
            onPanUpdate: (details) => _track(details.localPosition, size),
            onPanEnd: (_) => _leave(),
            onPanCancel: _leave,
            child: Stack(clipBehavior: Clip.none, children: [
              Positioned.fill(child: widget.chart),
              if (hover != null) ...[
                AnimatedPositioned(duration: move, left: x - .5, top: 0, bottom: widget.bottomAxis, child: Container(width: 1, color: const Color(0xFF888888))),
                for (final (color, fraction) in widget.dots(hover))
                  if (fraction != null)
                    AnimatedPositioned(
                      duration: move,
                      left: x - 4,
                      top: plotBottom * (1 - fraction) - 4,
                      child: Container(width: 8, height: 8, decoration: BoxDecoration(shape: BoxShape.circle, color: color)),
                    ),
                _TooltipCard(title: widget.title(hover), rows: widget.rows(hover), anchor: Offset(x, _pointer.dy), bounds: size),
              ],
            ]),
          ),
        );
      });
}

/// The web's ChartTooltipContent: the label on top, then a colored square, name and value per series.
class _TooltipCard extends StatelessWidget {
  const _TooltipCard({required this.title, required this.rows, required this.anchor, required this.bounds});
  final String title;
  final List<(Color, String, String)> rows;
  final Offset anchor;
  final Size bounds;

  static const _width = 148.0;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final height = 34.0 + rows.length * 20;
    // Left of the line when there is no room on the right, and kept inside the chart.
    final left = anchor.dx + 12 + _width > bounds.width ? anchor.dx - 12 - _width : anchor.dx + 12;
    final top = (anchor.dy - height / 2).clamp(0.0, (bounds.height - height).clamp(0.0, double.infinity));
    return Positioned(
      left: left,
      top: top,
      child: IgnorePointer(
        child: Container(
          width: _width,
          padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
          decoration: BoxDecoration(
            color: scheme.surface,
            borderRadius: BorderRadius.circular(radius),
            border: Border.all(color: scheme.border),
            boxShadow: [BoxShadow(color: Colors.black.withValues(alpha: .3), blurRadius: 12, offset: const Offset(0, 4))],
          ),
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, mainAxisSize: MainAxisSize.min, children: [
            Text(title, style: const TextStyle(fontSize: 12, fontWeight: FontWeight.w500)),
            for (final (color, name, value) in rows) ...[
              const SizedBox(height: 6),
              Row(children: [
                Container(width: 10, height: 10, decoration: BoxDecoration(color: color, borderRadius: BorderRadius.circular(2))),
                const SizedBox(width: 8),
                Expanded(child: Text(name, maxLines: 1, overflow: TextOverflow.ellipsis, style: TextStyle(fontSize: 12, color: scheme.mutedForeground))),
                Text(value, style: monoStyle(context).copyWith(fontSize: 12, fontWeight: FontWeight.w500)),
              ]),
            ],
          ]),
        ),
      ),
    );
  }
}

class _ModelRow extends StatelessWidget {
  const _ModelRow({required this.cells, this.header = false, this.tooltip = ''});
  final List<String> cells;
  final bool header;
  final String tooltip;
  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final style = TextStyle(fontSize: 13, color: header ? scheme.mutedForeground : null, fontWeight: header ? FontWeight.w500 : null, fontFeatures: const [FontFeature.tabularFigures()]);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 9),
      decoration: header ? BoxDecoration(border: Border(bottom: BorderSide(color: scheme.border))) : null,
      child: Row(children: [
        Expanded(flex: 3, child: Tooltip(message: header ? '' : cells[0], child: Text(cells[0], maxLines: 1, overflow: TextOverflow.ellipsis, style: style))),
        Expanded(flex: 2, child: Tooltip(message: tooltip, child: Text(cells[1], textAlign: TextAlign.right, style: style))),
        for (final cell in cells.skip(2)) Expanded(flex: 2, child: Text(cell, textAlign: TextAlign.right, maxLines: 1, style: style)),
      ]),
    );
  }
}
