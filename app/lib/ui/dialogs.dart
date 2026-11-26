import 'package:flutter/material.dart';

import '../api/client.dart';
import '../api/models.dart';
import '../state/ashira_state.dart';
import 'format.dart';
import 'theme.dart';
import 'transcript.dart';
import 'widgets.dart';
import 'window_frame.dart';
import 'workspace_image.dart';

String errorText(Object error, String fallback) => error is ApiException ? error.message : '$fallback：$error';

/// Title row like the web's DialogHeader: title, optional description, close on the right.
class PanelTitle extends StatelessWidget {
  const PanelTitle({super.key, required this.title, this.description});
  final String title;
  final String? description;
  @override
  Widget build(BuildContext context) => Container(
        padding: const EdgeInsets.fromLTRB(20, 14, 10, 14),
        decoration: BoxDecoration(border: Border(bottom: BorderSide(color: Theme.of(context).colorScheme.border))),
        child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Expanded(
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Padding(padding: const EdgeInsets.only(top: 6), child: Text(title, style: const TextStyle(fontSize: 16, fontWeight: FontWeight.w600))),
              if (description != null)
                Padding(
                  padding: const EdgeInsets.only(top: 4),
                  child: Text(description!, style: TextStyle(fontSize: 13, color: Theme.of(context).colorScheme.mutedForeground)),
                ),
            ]),
          ),
          IconButton(tooltip: '关闭', icon: const Icon(Icons.close), onPressed: () => Navigator.of(context).maybePop()),
        ]),
      );
}

/// Full-screen on phones, a fixed-size dialog on wide windows.
Future<T?> showPanel<T>(BuildContext context, {required String title, String? description, required Widget Function(BuildContext) builder, double width = 640, double? height}) {
  final size = MediaQuery.sizeOf(context);
  final page = Builder(
    builder: (context) => Material(
      color: Theme.of(context).dialogTheme.backgroundColor,
      child: SafeArea(
        child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
          PanelTitle(title: title, description: description),
          Expanded(child: builder(context)),
        ]),
      ),
    ),
  );
  if (size.width < 720 && !desktopWindow) return Navigator.of(context).push<T>(MaterialPageRoute(fullscreenDialog: true, builder: (_) => page));
  return showDialog<T>(
    context: context,
    builder: (_) => Dialog(
      clipBehavior: Clip.antiAlias,
      child: SizedBox(width: width, height: (height ?? size.height * .8).clamp(0, size.height - 48), child: page),
    ),
  );
}

// ---------------------------------------------------------------- models

Future<void> showModelPicker(BuildContext context, AshiraState state) => showPanel(
      context,
      title: '切换模型',
      builder: (_) => _ModelPicker(state),
      width: 680,
      height: 580,
    );

class _ModelPicker extends StatefulWidget {
  const _ModelPicker(this.state);
  final AshiraState state;
  @override
  State<_ModelPicker> createState() => _ModelPickerState();
}

class _ModelPickerState extends State<_ModelPicker> {
  ModelCatalog? _catalog;
  String _error = '';
  String _query = '';
  String _level = '默认';
  String _group = '';
  /// The model the level row describes: the current one, then whichever was tapped.
  String _picked = '';
  late bool _fast = widget.state.status?.fast ?? false;

  String get _current => widget.state.status?.model ?? '';

  @override
  void initState() {
    super.initState();
    widget.state.client.models().then((catalog) {
      if (!mounted) return;
      final provider = _current.split('/').first;
      setState(() {
        _catalog = catalog;
        _level = _levelOf(_current, catalog.levels);
        _picked = _base(_current);
        _group = catalog.models.any((model) => model.provider == provider) ? provider : '';
      });
    }).catchError((Object error) {
      if (mounted) setState(() => _error = errorText(error, '无法加载模型'));
    });
  }

  /// Like the web: picking a model or a level applies at once and keeps the panel open,
  /// so the level can follow the model.
  void _apply() {
    if (_picked.isEmpty) return;
    // The start page shows the name before any session exists to resolve it.
    final item = _catalog?.models.where((model) => model.selector == _picked).firstOrNull;
    widget.state.setModel(_level == '默认' ? _picked : '$_picked:$_level', name: item == null ? null : (item.name ?? item.id));
  }

  void _choose(ModelItem model, List<String> levels) {
    setState(() {
      _picked = model.selector;
      // A level the new model doesn't accept falls back to 默认 instead of being sent.
      if (!_levelsFor(model, levels).contains(_level)) _level = '默认';
    });
    _apply();
  }

  static String _levelOf(String selector, List<String> levels) {
    final split = selector.lastIndexOf(':');
    if (split <= 0) return '默认';
    final tail = selector.substring(split + 1);
    return levels.contains(tail) ? tail : '默认';
  }

  String _base(String selector) {
    final split = selector.lastIndexOf(':');
    if (split <= 0) return selector;
    return (_catalog?.levels ?? const []).contains(selector.substring(split + 1)) ? selector.substring(0, split) : selector;
  }

  /// Levels the model accepts, like the TUI; unknown support (no models.dev data) offers all.
  static List<String> _levelsFor(ModelItem? model, List<String> levels) {
    final efforts = model?.efforts ?? const [];
    return levels.where((level) => level == '默认' || efforts.isEmpty || efforts.contains(level) || (level == 'off' && efforts.contains('none'))).toList();
  }

  static String _rate(double value) => value == value.roundToDouble() ? value.toInt().toString() : double.parse(value.toStringAsFixed(4)).toString();

  @override
  Widget build(BuildContext context) {
    final catalog = _catalog;
    final scheme = Theme.of(context).colorScheme;
    if (_error.isNotEmpty) return Center(child: Text(_error, style: TextStyle(color: scheme.error)));
    if (catalog == null) return const Center(child: SizedBox.square(dimension: 18, child: CircularProgressIndicator()));
    final terms = _query.trim().toLowerCase().split(RegExp(r'\s+')).where((term) => term.isNotEmpty);
    final matched = catalog.models.where((model) {
      final hay = '${model.selector} ${model.name ?? ''} ${model.provider}'.toLowerCase();
      return terms.every(hay.contains);
    }).toList();
    final counts = <String, int>{};
    for (final model in matched) {
      counts[model.provider] = (counts[model.provider] ?? 0) + 1;
    }
    final groups = counts.keys.toList()..sort();
    final shown = _group.isEmpty ? matched : matched.where((model) => model.provider == _group).toList();
    final missing = _current.isNotEmpty && !catalog.models.any((model) => model.selector == _base(_current));
    final narrow = MediaQuery.sizeOf(context).width < 720;
    final small = TextStyle(fontSize: 12, color: scheme.mutedForeground);

    Widget providerRow(String label, int count, String value) => CompactRow(
          selected: _group == value,
          onTap: () => setState(() => _group = value),
          padding: const EdgeInsets.symmetric(horizontal: 10),
          child: Row(children: [
            Expanded(child: Text(label, maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 13))),
            Text('$count', style: small),
          ]),
        );

    final list = ListView(children: [
      if (missing)
        Container(
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
          decoration: BoxDecoration(color: scheme.muted, border: Border(bottom: BorderSide(color: scheme.border))),
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text('当前：$_current', style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w500)),
            const SizedBox(height: 2),
            Text('不在模型目录中，可能已从配置移除；选择其他模型后会替换它。', style: small),
          ]),
        ),
      for (final model in shown)
        Material(
          color: _picked == model.selector ? scheme.muted : Colors.transparent,
          child: InkWell(
            onTap: () => _choose(model, catalog.levels),
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
              decoration: BoxDecoration(border: Border(bottom: BorderSide(color: scheme.border))),
              child: Row(children: [
                Expanded(
                  child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                    Text(model.name ?? model.id, maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w500)),
                    const SizedBox(height: 1),
                    Text(model.selector, maxLines: 1, overflow: TextOverflow.ellipsis, style: small),
                  ]),
                ),
                const SizedBox(width: 12),
                Column(crossAxisAlignment: CrossAxisAlignment.end, children: [
                  Text('${compact(model.context)} 上下文', style: small),
                  if (model.input != null && model.output != null) Text('\$${_rate(model.input!)} / \$${_rate(model.output!)}', style: small),
                ]),
              ]),
            ),
          ),
        ),
      if (shown.isEmpty) Padding(padding: const EdgeInsets.all(24), child: Center(child: Text('没有匹配的模型', style: small))),
    ]);

    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 16, 20, 20),
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        TextField(decoration: const InputDecoration(hintText: '搜索服务商或模型'), onChanged: (value) => setState(() => _query = value)),
        const SizedBox(height: 12),
        Row(children: [
          Expanded(
            child: Wrap(spacing: 4, runSpacing: 4, children: [
              for (final level in _levelsFor(catalog.models.where((model) => model.selector == _picked).firstOrNull, catalog.levels))
                Pill(
                  label: level,
                  selected: level == _level,
                  onTap: () {
                    setState(() => _level = level);
                    _apply();
                  },
                ),
            ]),
          ),
          // OpenAI fast only exists for GPT models on the OpenAI wire formats.
          if (catalog.models.any((model) => model.selector == _picked && model.fast)) ...[
            const SizedBox(width: 12),
            const Text('Fast', style: TextStyle(fontSize: 13, fontWeight: FontWeight.w500)),
            const SizedBox(width: 4),
            Toggle(
              label: 'Fast',
              value: _fast,
              onChanged: (value) {
                setState(() => _fast = value);
                widget.state.setFast(value);
              },
            ),
          ],
        ]),
        const SizedBox(height: 12),
        Expanded(
          child: Container(
            decoration: BoxDecoration(border: Border.all(color: scheme.border), borderRadius: BorderRadius.circular(radius)),
            clipBehavior: Clip.antiAlias,
            child: narrow
                ? Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                    Container(
                      decoration: BoxDecoration(border: Border(bottom: BorderSide(color: scheme.border))),
                      padding: const EdgeInsets.symmetric(horizontal: 12),
                      child: DropdownButtonHideUnderline(
                        child: DropdownButton<String>(
                          value: _group,
                          isExpanded: true,
                          style: TextStyle(fontSize: 14, color: scheme.onSurface),
                          dropdownColor: scheme.card,
                          items: [
                            DropdownMenuItem(value: '', child: Text('全部服务商（${matched.length}）')),
                            for (final group in groups) DropdownMenuItem(value: group, child: Text('$group（${counts[group]}）')),
                          ],
                          onChanged: (value) => setState(() => _group = value ?? ''),
                        ),
                      ),
                    ),
                    Expanded(child: list),
                  ])
                : Row(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                    Container(
                      width: 148,
                      decoration: BoxDecoration(border: Border(right: BorderSide(color: scheme.border))),
                      child: ListView(padding: const EdgeInsets.all(4), children: [
                        providerRow('全部', matched.length, ''),
                        for (final group in groups) providerRow(group, counts[group]!, group),
                      ]),
                    ),
                    Expanded(child: list),
                  ]),
          ),
        ),
      ]),
    );
  }
}

// ---------------------------------------------------------------- project picker

Future<String?> showProjectPicker(BuildContext context, AshiraState state) =>
    showPanel<String>(context, title: '选择目录', builder: (_) => _ProjectPicker(state.client, state.cwd), width: 560, height: 560);

class _ProjectPicker extends StatefulWidget {
  const _ProjectPicker(this.client, this.start);
  final AshiraClient client;
  final String start;
  @override
  State<_ProjectPicker> createState() => _ProjectPickerState();
}

class _ProjectPickerState extends State<_ProjectPicker> {
  BrowseListing? _listing;
  bool _loading = false;
  String _error = '';

  @override
  void initState() {
    super.initState();
    _load(widget.start);
  }

  Future<void> _load(String path) async {
    setState(() {
      _loading = true;
      _error = '';
    });
    try {
      final listing = await widget.client.browse(path);
      if (mounted) setState(() => _listing = listing);
    } catch (error) {
      if (mounted) setState(() => _error = errorText(error, '无法打开目录'));
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  /// Breadcrumb segments with the path each one opens, like the web picker.
  List<({String name, String path})> _crumbs(String value) {
    if (value.isEmpty) return const [];
    final sep = value.contains('\\') ? '\\' : '/';
    final crumbs = <({String name, String path})>[];
    var acc = '';
    for (final part in value.split(RegExp(r'[\\/]')).where((part) => part.isNotEmpty)) {
      acc = acc.isEmpty ? (part.endsWith(':') ? '$part$sep' : (value.startsWith('/') ? '/$part' : part)) : '${acc.replaceFirst(RegExp(r'[\\/]$'), '')}$sep$part';
      crumbs.add((name: part, path: acc));
    }
    return crumbs;
  }

  @override
  Widget build(BuildContext context) {
    final listing = _listing;
    final scheme = Theme.of(context).colorScheme;
    final crumbs = _crumbs(listing?.path ?? '');
    final name = crumbs.isEmpty ? '此目录' : crumbs.last.name;
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 12, 20, 20),
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        SizedBox(
          height: 32,
          child: ListView(scrollDirection: Axis.horizontal, children: [
            for (final (index, crumb) in crumbs.indexed) ...[
              if (index > 0) Icon(Icons.chevron_right, size: 14, color: scheme.mutedForeground),
              TextButton(
                onPressed: _loading || index == crumbs.length - 1 ? null : () => _load(crumb.path),
                style: TextButton.styleFrom(
                  padding: const EdgeInsets.symmetric(horizontal: 6),
                  minimumSize: const Size(0, 28),
                  backgroundColor: index == crumbs.length - 1 ? scheme.muted : null,
                  disabledForegroundColor: index == crumbs.length - 1 ? scheme.onSurface : null,
                ),
                child: Text(crumb.name),
              ),
            ],
          ]),
        ),
        if (_error.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 8), child: Text(_error, style: TextStyle(fontSize: 13, color: scheme.error))),
        const SizedBox(height: 12),
        Expanded(
          child: Container(
            decoration: BoxDecoration(border: Border.all(color: scheme.border), borderRadius: BorderRadius.circular(radius)),
            clipBehavior: Clip.antiAlias,
            child: _loading
                ? const Center(child: SizedBox.square(dimension: 18, child: CircularProgressIndicator()))
                : (listing?.entries.isEmpty ?? true)
                    ? Center(child: Text('没有子目录，可以直接使用当前目录', style: TextStyle(fontSize: 13, color: scheme.mutedForeground)))
                    : ListView(padding: const EdgeInsets.all(4), children: [
                        for (final entry in listing!.entries)
                          CompactRow(
                            onTap: () => _load(entry.path),
                            padding: const EdgeInsets.symmetric(horizontal: 10),
                            child: Row(children: [
                              Icon(Icons.folder_outlined, size: 15, color: scheme.mutedForeground),
                              const SizedBox(width: 8),
                              Expanded(child: Text(entry.name, maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 13))),
                              Icon(Icons.chevron_right, size: 14, color: scheme.mutedForeground),
                            ]),
                          ),
                      ]),
          ),
        ),
        const SizedBox(height: 16),
        Row(children: [
          Expanded(child: Text(listing?.path ?? '', maxLines: 1, overflow: TextOverflow.ellipsis, style: TextStyle(fontSize: 12, color: scheme.mutedForeground))),
          const SizedBox(width: 12),
          ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 260),
            child: FilledButton(
              onPressed: listing == null || _loading ? null : () => Navigator.of(context).pop(listing.path),
              child: Text('在 $name 开始', maxLines: 1, overflow: TextOverflow.ellipsis),
            ),
          ),
        ]),
      ]),
    );
  }
}

// ---------------------------------------------------------------- subagent

/// `images` loads workspace pictures for the parent session (the panel is a new route,
/// outside the transcript's [WorkspaceImages]).
Future<void> showSubagent(BuildContext context, AshiraClient client, String id, Uri Function(String path) images) =>
    showPanel(context, title: '子代理对话', builder: (_) => WorkspaceImages(uri: images, child: _Subagent(client, id)), width: 960);

class _Subagent extends StatelessWidget {
  const _Subagent(this.client, this.id);
  final AshiraClient client;
  final String id;

  static String _task(String text) {
    const mark = '\nTask:\n';
    final at = text.lastIndexOf(mark);
    return at >= 0 ? text.substring(at + mark.length).trim() : text;
  }

  @override
  Widget build(BuildContext context) => FutureBuilder(
        future: client.subagent(id),
        builder: (context, snapshot) {
          if (snapshot.hasError) return Center(child: Text(errorText(snapshot.error!, '无法读取子任务'), style: TextStyle(color: Theme.of(context).colorScheme.error)));
          final agent = snapshot.data;
          if (agent == null) return const Center(child: SizedBox.square(dimension: 18, child: CircularProgressIndicator()));
          final blocks = agent.blocks.isNotEmpty
              ? [for (final block in agent.blocks) block is UserBlock ? UserBlock(_task(block.text), block.images, block.files) : block]
              : agent.report.isNotEmpty
                  ? <Block>[AssistantBlock(parts: [TextPart(agent.report)])]
                  : <Block>[];
          return Transcript(blocks: blocks);
        },
      );
}

// ---------------------------------------------------------------- small dialogs

/// Compact dialog in the web's style: title, description, body, right-aligned actions.
class SmallDialog extends StatelessWidget {
  const SmallDialog({super.key, required this.title, this.description, this.body, required this.actions});
  final String title;
  final String? description;
  final Widget? body;
  final List<Widget> actions;
  @override
  Widget build(BuildContext context) => Dialog(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 440),
          child: Padding(
            padding: const EdgeInsets.all(24),
            child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.stretch, children: [
              Text(title, style: const TextStyle(fontSize: 16, fontWeight: FontWeight.w600)),
              if (description != null)
                Padding(
                  padding: const EdgeInsets.only(top: 6),
                  child: Text(description!, style: TextStyle(fontSize: 13, color: Theme.of(context).colorScheme.mutedForeground)),
                ),
              if (body != null) Padding(padding: const EdgeInsets.only(top: 16), child: body),
              const SizedBox(height: 20),
              Wrap(alignment: WrapAlignment.end, spacing: 8, runSpacing: 8, children: actions),
            ]),
          ),
        ),
      );
}

Future<void> showConfirm(BuildContext context, AshiraState state, ConfirmRequest request) async {
  final decision = await showDialog<String>(
    context: context,
    barrierDismissible: false,
    builder: (context) => SmallDialog(
      title: '确认 ${request.name}',
      description: request.reason.isEmpty ? null : request.reason,
      body: Container(
        constraints: const BoxConstraints(maxHeight: 200),
        padding: const EdgeInsets.all(12),
        decoration: BoxDecoration(color: Theme.of(context).colorScheme.muted, borderRadius: BorderRadius.circular(radius)),
        child: SingleChildScrollView(child: SelectableText(request.command, style: monoStyle(context).copyWith(fontSize: 12, height: 1.5))),
      ),
      actions: [
        OutlinedButton(onPressed: () => Navigator.pop(context, 'no'), child: const Text('拒绝')),
        OutlinedButton(onPressed: () => Navigator.pop(context, 'always'), child: Text(request.always)),
        FilledButton(onPressed: () => Navigator.pop(context, 'yes'), child: const Text('允许')),
      ],
    ),
  );
  if (decision != null && state.confirm?.id == request.id) await state.decide(decision);
}

Future<String?> askText(BuildContext context, {required String title, String? description, String initial = '', String hint = '', bool obscure = false, String action = '保存'}) =>
    showDialog<String>(
      context: context,
      builder: (_) => _AskText(title: title, description: description, initial: initial, hint: hint, obscure: obscure, action: action),
    );

/// Owns its controller so it outlives the close animation; disposing it when the
/// Future completes would tear it down while the field is still on screen.
class _AskText extends StatefulWidget {
  const _AskText({required this.title, required this.description, required this.initial, required this.hint, required this.obscure, required this.action});
  final String title, initial, hint, action;
  final String? description;
  final bool obscure;
  @override
  State<_AskText> createState() => _AskTextState();
}

class _AskTextState extends State<_AskText> {
  late final _controller = TextEditingController(text: widget.initial);

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => SmallDialog(
        title: widget.title,
        description: widget.description,
        body: TextField(
          controller: _controller,
          autofocus: true,
          obscureText: widget.obscure,
          decoration: InputDecoration(hintText: widget.hint),
          onSubmitted: (value) => Navigator.pop(context, value),
        ),
        actions: [
          OutlinedButton(onPressed: () => Navigator.pop(context), child: const Text('取消')),
          FilledButton(onPressed: () => Navigator.pop(context, _controller.text), child: Text(widget.action)),
        ],
      );
}
