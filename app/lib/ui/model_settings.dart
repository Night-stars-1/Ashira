import 'package:flutter/material.dart';

import '../api/client.dart';
import '../api/models.dart';
import 'dialogs.dart';
import 'select_field.dart';
import 'theme.dart';
import 'widgets.dart';

const _apiLabels = {
  'openai-responses': 'OpenAI Responses',
  'openai-completions': 'OpenAI Chat Completions',
  'anthropic-messages': 'Anthropic Messages',
};
const _keyKinds = [('literal', '直接填写'), ('env', '环境变量'), ('command', '命令'), ('none', '不需要')];
/// Role, label and what an empty value means.
const _roleRows = [
  ('default', '新会话', '未设置（用第一个可用模型）'),
  ('task', '子代理', '跟随当前会话的模型'),
  ('compact', '压缩上下文', '跟随当前会话的模型'),
  ('title', '生成标题', '跟随当前会话的模型'),
  ('memory', '整理记忆', '跟随当前会话的模型'),
  ('image', '生成图片', '未设置（Responses 模型用服务商自带的图片生成）'),
];

/// Settings → 模型: model roles and the providers in models.yml.
class ModelSettings extends StatefulWidget {
  const ModelSettings({super.key, required this.client});
  final AshiraClient client;
  @override
  State<ModelSettings> createState() => _ModelSettingsState();
}

class _ModelSettingsState extends State<ModelSettings> {
  ModelConfig? _config;
  List<ModelItem> _models = const [];
  List<String> _levels = const [];
  String _loadError = '';
  String _notice = '';
  final _roles = {for (final role in modelRoles) role: (model: '', level: '')};
  bool _savingRoles = false;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final results = await Future.wait([widget.client.modelConfig(), widget.client.models().then<Object?>((value) => value).catchError((_) => null)]);
      if (!mounted) return;
      final config = results[0] as ModelConfig;
      final catalog = results[1] as ModelCatalog?;
      setState(() {
        if (catalog != null) {
          _models = catalog.models;
          _levels = catalog.levels.where((level) => level != '默认').toList();
        }
        _config = config;
        for (final role in modelRoles) {
          _roles[role] = _split(config.roles[role] ?? '');
        }
        _loadError = '';
      });
    } catch (error) {
      if (mounted) setState(() => _loadError = errorText(error, '读取模型配置失败'));
    }
  }

  /// `provider/model:level` → parts; a trailing segment that isn't a level is part of the id.
  ({String model, String level}) _split(String selector) {
    final at = selector.lastIndexOf(':');
    final tail = at > 0 ? selector.substring(at + 1) : '';
    return _levels.contains(tail) ? (model: selector.substring(0, at), level: tail) : (model: selector, level: '');
  }

  String _joined(String role) {
    final value = _roles[role]!;
    return value.model.isEmpty ? '' : value.level.isEmpty ? value.model : '${value.model}:${value.level}';
  }

  bool get _rolesDirty => _config != null && modelRoles.any((role) => _joined(role) != (_config!.roles[role] ?? ''));

  /// Levels the model accepts; unknown support offers all, like the model picker.
  List<String> _roleLevels(String selector) {
    final efforts = _models.where((model) => model.selector == selector).firstOrNull?.efforts ?? const [];
    return _levels.where((level) => efforts.isEmpty || efforts.contains(level) || (level == 'off' && efforts.contains('none'))).toList();
  }

  Future<void> _saveRoles() async {
    setState(() {
      _savingRoles = true;
      _notice = '';
    });
    try {
      await widget.client.saveRoles({for (final role in modelRoles) role: _joined(role)});
      await _load();
    } catch (error) {
      if (mounted) setState(() => _notice = errorText(error, '保存失败'));
    } finally {
      if (mounted) setState(() => _savingRoles = false);
    }
  }

  Future<void> _edit([ProviderConfig? provider]) async {
    final config = _config;
    if (config == null) return;
    final saved = await showDialog<bool>(context: context, builder: (_) => _ProviderForm(client: widget.client, apis: config.apis, models: _models, editing: provider));
    if (saved == true) await _load();
  }

  Future<void> _remove(ProviderConfig provider) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => SmallDialog(
        title: '删除 ${provider.id}？',
        description: '会从模型配置文件里删除这个服务商及其高级设置。用它作默认模型的地方需要重新选择。',
        actions: [
          OutlinedButton(onPressed: () => Navigator.pop(context, false), child: const Text('取消')),
          FilledButton(
            style: FilledButton.styleFrom(backgroundColor: Theme.of(context).colorScheme.error, foregroundColor: Theme.of(context).colorScheme.onError),
            onPressed: () => Navigator.pop(context, true),
            child: const Text('删除'),
          ),
        ],
      ),
    );
    if (confirmed != true || !mounted) return;
    setState(() => _notice = '');
    try {
      await widget.client.removeProvider(provider.id);
      await _load();
    } catch (error) {
      if (mounted) setState(() => _notice = errorText(error, '删除失败'));
    }
  }

  static String _keyLabel(ProviderKey key) => switch (key.kind) {
        'env' => key.unset ? '环境变量 ${key.value}（未设置）' : '环境变量 ${key.value}',
        'command' => '命令获取',
        'literal' => '已填写 Key',
        'none' => '无需 Key',
        _ => '未填写 Key',
      };

  static String _statusLabel(ProviderStatus? status) => switch (status?.state) {
        null => '未加载',
        'pending' => '获取中',
        'failed' => '失败',
        _ => '${status!.models} 个模型',
      };

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final config = _config;
    if (_loadError.isNotEmpty) return Text(_loadError, style: TextStyle(fontSize: 13, color: scheme.error));
    if (config == null) return const Center(child: Padding(padding: EdgeInsets.all(24), child: SizedBox.square(dimension: 18, child: CircularProgressIndicator())));
    final small = TextStyle(fontSize: 12, color: scheme.mutedForeground);
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      if (config.loadError != null)
        Container(
          margin: const EdgeInsets.only(bottom: 12),
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
          decoration: BoxDecoration(border: Border.all(color: scheme.error.withValues(alpha: .5)), borderRadius: BorderRadius.circular(radius)),
          child: SelectableText(config.loadError!, style: TextStyle(fontSize: 13, color: scheme.error)),
        ),
      if (_notice.isNotEmpty) Padding(padding: const EdgeInsets.only(bottom: 12), child: Text(_notice, style: TextStyle(fontSize: 13, color: scheme.error))),
      Container(
        padding: const EdgeInsets.all(12),
        decoration: BoxDecoration(border: Border.all(color: scheme.border), borderRadius: BorderRadius.circular(radius)),
        child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
          const Text('模型分工', style: TextStyle(fontSize: 14, fontWeight: FontWeight.w500)),
          for (final (role, label, placeholder) in _roleRows) ...[
            const SizedBox(height: 10),
            Text(label, style: small),
            const SizedBox(height: 4),
            Row(children: [
              Expanded(
                child: ModelSearchField(
                  models: _models,
                  value: _roles[role]!.model,
                  placeholder: placeholder,
                  onChanged: (model) => setState(() => _roles[role] = (model: model, level: model.isEmpty ? '' : _roles[role]!.level)),
                ),
              ),
              // Image models do not think.
              if (role != 'image') ...[
                const SizedBox(width: 8),
                SizedBox(
                  width: 112,
                  child: SelectField<String>(
                    label: '$label思考强度',
                    enabled: _roles[role]!.model.isNotEmpty,
                    value: _roles[role]!.level,
                    onChanged: (level) => setState(() => _roles[role] = (model: _roles[role]!.model, level: level)),
                    options: [('', '默认强度'), for (final level in _roleLevels(_roles[role]!.model)) (level, level)],
                  ),
                ),
              ],
            ]),
          ],
          const SizedBox(height: 12),
          Align(
            alignment: Alignment.centerRight,
            child: FilledButton(onPressed: _rolesDirty && !_savingRoles ? _saveRoles : null, child: Text(_savingRoles ? '保存中…' : '保存')),
          ),
        ]),
      ),
      const SizedBox(height: 16),
      Row(children: [
        const Expanded(child: Text('服务商', style: TextStyle(fontSize: 14, fontWeight: FontWeight.w500))),
        FilledButton(onPressed: _edit, child: const Text('添加')),
      ]),
      const SizedBox(height: 8),
      if (config.providers.isEmpty) const EmptyState(title: '没有服务商', description: '添加一个服务商，填写接口地址和 API Key 后就能选择它的模型。'),
      for (final provider in config.providers)
        Padding(
          padding: const EdgeInsets.only(bottom: 8),
          child: Container(
            padding: const EdgeInsets.fromLTRB(12, 8, 4, 8),
            decoration: BoxDecoration(border: Border.all(color: scheme.border), borderRadius: BorderRadius.circular(radius)),
            child: Row(children: [
              Expanded(
                child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                  Text(provider.id, maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w500)),
                  Tooltip(
                    message: provider.baseUrl,
                    child: Text('${_apiLabels[provider.api] ?? (provider.api.isEmpty ? '未设置接口' : provider.api)} · ${_keyLabel(provider.key)}', maxLines: 1, overflow: TextOverflow.ellipsis, style: small),
                  ),
                  if (provider.status?.detail != null)
                    Padding(padding: const EdgeInsets.only(top: 2), child: Text(provider.status!.detail!, maxLines: 2, overflow: TextOverflow.ellipsis, style: TextStyle(fontSize: 12, color: scheme.error))),
                ]),
              ),
              StatusTag(_statusLabel(provider.status), destructive: provider.status?.state == 'failed'),
              const SizedBox(width: 4),
              TextButton(onPressed: () => _edit(provider), child: const Text('编辑')),
              TextButton(style: TextButton.styleFrom(foregroundColor: scheme.error), onPressed: () => _remove(provider), child: const Text('删除')),
            ]),
          ),
        ),
      if (config.warnings.isNotEmpty) Text(config.warnings.join('\n'), style: small),
    ]);
  }
}

/// Searchable model selector grouped by provider; '' is the "none" choice.
class ModelSearchField extends StatelessWidget {
  const ModelSearchField({super.key, required this.models, required this.value, required this.placeholder, required this.onChanged});
  final List<ModelItem> models;
  final String value, placeholder;
  final ValueChanged<String> onChanged;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final selected = models.where((model) => model.selector == value).firstOrNull;
    final unlisted = value.isNotEmpty && selected == null;
    return OutlinedButton(
      style: OutlinedButton.styleFrom(alignment: Alignment.centerLeft, padding: const EdgeInsets.symmetric(horizontal: 10)),
      onPressed: () async {
        final picked = await showPanel<String>(
          context,
          title: '选择模型',
          width: 560,
          builder: (_) => _ModelSearch(models: models, value: value, placeholder: placeholder),
        );
        if (picked != null) onChanged(picked);
      },
      child: Row(children: [
        Expanded(
          child: value.isEmpty
              ? Text(placeholder, maxLines: 1, overflow: TextOverflow.ellipsis, style: TextStyle(color: scheme.mutedForeground, fontWeight: FontWeight.normal))
              : Text.rich(
                  TextSpan(children: [
                    TextSpan(text: value, style: const TextStyle(fontWeight: FontWeight.normal)),
                    if (selected?.name != null) TextSpan(text: '  ${selected!.name}', style: TextStyle(fontSize: 12, color: scheme.mutedForeground, fontWeight: FontWeight.normal)),
                    if (unlisted) TextSpan(text: '  不在模型列表里', style: TextStyle(fontSize: 12, color: scheme.error, fontWeight: FontWeight.normal)),
                  ]),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
        ),
        Icon(Icons.unfold_more, size: 16, color: scheme.mutedForeground),
      ]),
    );
  }
}

class _ModelSearch extends StatefulWidget {
  const _ModelSearch({required this.models, required this.value, required this.placeholder});
  final List<ModelItem> models;
  final String value, placeholder;
  @override
  State<_ModelSearch> createState() => _ModelSearchState();
}

class _ModelSearchState extends State<_ModelSearch> {
  String _query = '';

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final terms = _query.trim().toLowerCase().split(RegExp(r'\s+')).where((term) => term.isNotEmpty);
    final matched = widget.models.where((model) => terms.every('${model.selector} ${model.name ?? ''}'.toLowerCase().contains)).toList();
    final groups = <String, List<ModelItem>>{};
    for (final model in matched) {
      groups.putIfAbsent(model.provider, () => []).add(model);
    }
    Widget row(String value, Widget label) => CompactRow(
          selected: value == widget.value,
          onTap: () => Navigator.pop(context, value),
          padding: const EdgeInsets.symmetric(horizontal: 10),
          child: Row(children: [Expanded(child: label), if (value == widget.value) const Icon(Icons.check, size: 16)]),
        );
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      Padding(
        padding: const EdgeInsets.fromLTRB(16, 12, 16, 8),
        child: TextField(autofocus: true, decoration: const InputDecoration(hintText: '搜索模型', prefixIcon: Icon(Icons.search, size: 18)), onChanged: (value) => setState(() => _query = value)),
      ),
      Expanded(
        child: ListView(padding: const EdgeInsets.fromLTRB(8, 0, 8, 12), children: [
          if (_query.trim().isEmpty) row('', Text(widget.placeholder, style: TextStyle(fontSize: 13, color: scheme.mutedForeground))),
          for (final entry in groups.entries) ...[
            Padding(padding: const EdgeInsets.fromLTRB(10, 10, 10, 4), child: Text(entry.key, style: TextStyle(fontSize: 12, color: scheme.mutedForeground))),
            for (final model in entry.value)
              row(
                model.selector,
                Text.rich(
                  TextSpan(children: [
                    TextSpan(text: model.id, style: const TextStyle(fontSize: 13)),
                    if (model.name != null && model.name != model.id) TextSpan(text: '  ${model.name}', style: TextStyle(fontSize: 12, color: scheme.mutedForeground)),
                  ]),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
          ],
          if (matched.isEmpty) Padding(padding: const EdgeInsets.all(16), child: Text('没有匹配的模型', textAlign: TextAlign.center, style: TextStyle(fontSize: 13, color: scheme.mutedForeground))),
        ]),
      ),
    ]);
  }
}

/// Add or edit one provider. Fetching lists the connection's models without saving.
class _ProviderForm extends StatefulWidget {
  const _ProviderForm({required this.client, required this.apis, required this.models, this.editing});
  final AshiraClient client;
  final List<String> apis;
  /// Every known model, for this provider's image model.
  final List<ModelItem> models;
  final ProviderConfig? editing;
  @override
  State<_ProviderForm> createState() => _ProviderFormState();
}

class _ProviderFormState extends State<_ProviderForm> {
  late final _id = TextEditingController(text: widget.editing?.id ?? '');
  late final _baseUrl = TextEditingController(text: widget.editing?.baseUrl ?? '');
  late final _key = TextEditingController(text: _shownKey(widget.editing?.key));
  late final _models = TextEditingController(text: widget.editing?.models.join('\n') ?? '');
  final _search = TextEditingController();
  late String _api = widget.editing?.api.isNotEmpty == true ? widget.editing!.api : 'openai-completions';
  late String _keyKind = switch (widget.editing?.key.kind) { null || 'missing' => 'literal', final kind => kind };
  late bool _discovery = widget.editing?.discovery ?? true;
  /// This provider's own `generate_image` model; '' uses modelRoles.image.
  late String _imageModel = widget.editing?.imageModel ?? '';
  List<String>? _fetched;
  bool _fetching = false;
  bool _saving = false;
  String _fetchError = '';
  String _error = '';

  /// env/command values are shown; a stored literal never is.
  static String _shownKey(ProviderKey? key) => key != null && (key.kind == 'env' || key.kind == 'command') ? key.value : '';

  @override
  void initState() {
    super.initState();
    for (final controller in [_baseUrl, _models, _search]) {
      controller.addListener(() => setState(() {}));
    }
  }

  @override
  void dispose() {
    for (final controller in [_id, _baseUrl, _key, _models, _search]) {
      controller.dispose();
    }
    super.dispose();
  }

  List<String> get _listed => _models.text.split(RegExp(r'[\n,]')).map((model) => model.trim()).where((model) => model.isNotEmpty).toList();

  /// True when the stored key can be kept by leaving the field empty.
  bool get _keepsKey {
    final stored = widget.editing?.key.kind;
    return stored != null && stored == _keyKind && stored != 'none' && stored != 'missing';
  }

  ProviderForm get _form => ProviderForm(
        id: _id.text.trim(),
        api: _api,
        baseUrl: _baseUrl.text.trim(),
        keyKind: _keyKind,
        keyValue: _key.text.trim(),
        discovery: _discovery,
        imageModel: _imageModel,
        models: _listed,
      );

  void _setKind(String kind) {
    final stored = widget.editing?.key;
    setState(() {
      _keyKind = kind;
      _key.text = stored != null && stored.kind == kind ? _shownKey(stored) : '';
    });
  }

  Future<void> _fetch() async {
    setState(() {
      _fetching = true;
      _fetchError = '';
    });
    try {
      final models = await widget.client.probeModels(widget.editing?.id ?? '', _form);
      if (!mounted) return;
      setState(() {
        _fetched = models;
        if (models.isEmpty) _fetchError = '接口没有返回任何模型';
      });
    } catch (error) {
      if (mounted) setState(() => _fetchError = errorText(error, '获取失败'));
    } finally {
      if (mounted) setState(() => _fetching = false);
    }
  }

  void _toggle(String id, bool on) {
    final current = _listed;
    _models.text = (on ? [...current.where((model) => model != id), id] : current.where((model) => model != id)).join('\n');
  }

  Future<void> _save() async {
    setState(() {
      _saving = true;
      _error = '';
    });
    try {
      await widget.client.saveProvider(widget.editing?.id ?? '', _form);
      if (mounted) Navigator.pop(context, true);
    } catch (error) {
      if (mounted) setState(() => _error = errorText(error, '保存失败'));
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final small = TextStyle(fontSize: 12, color: scheme.mutedForeground);
    Widget label(String text, {bool first = false}) => Padding(padding: EdgeInsets.only(bottom: 4, top: first ? 0 : 12), child: Text(text, style: small));
    final fetched = _fetched ?? const [];
    final terms = _search.text.trim().toLowerCase().split(RegExp(r'\s+')).where((term) => term.isNotEmpty);
    final shown = fetched.where((id) => terms.every(id.toLowerCase().contains)).toList();
    final listed = _listed;
    return SmallDialog(
      title: widget.editing == null ? '添加服务商' : '编辑 ${widget.editing!.id}',
      body: ConstrainedBox(
        constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .6),
        child: SingleChildScrollView(
          child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
            label('服务商 ID', first: true),
            TextField(controller: _id, decoration: const InputDecoration(hintText: 'openai'), autocorrect: false),
            label('接口类型'),
            SelectField<String>(
              label: '接口类型',
              value: _api,
              onChanged: (api) => setState(() => _api = api),
              options: [for (final api in widget.apis) (api, _apiLabels[api] ?? api)],
            ),
            label('Base URL'),
            TextField(controller: _baseUrl, decoration: const InputDecoration(hintText: 'https://api.openai.com/v1'), autocorrect: false),
            label('API Key'),
            Wrap(spacing: 4, runSpacing: 4, children: [
              for (final (kind, name) in _keyKinds) Pill(label: name, selected: _keyKind == kind, onTap: () => _setKind(kind)),
            ]),
            if (_keyKind != 'none') ...[
              const SizedBox(height: 6),
              TextField(
                controller: _key,
                obscureText: _keyKind == 'literal',
                autocorrect: false,
                decoration: InputDecoration(
                  hintText: switch (_keyKind) {
                    'literal' when _keepsKey => '已保存，留空则不修改',
                    'env' => 'OPENAI_API_KEY',
                    'command' => 'op read op://vault/openai/key',
                    _ => 'sk-...',
                  },
                ),
              ),
            ],
            const SizedBox(height: 12),
            Row(children: [
              const Expanded(child: Text('自动获取模型列表', style: TextStyle(fontSize: 13))),
              Toggle(label: '自动获取模型列表', value: _discovery, onChanged: (value) => setState(() => _discovery = value)),
            ]),
            Padding(
              padding: const EdgeInsets.only(top: 12, bottom: 4),
              child: Row(children: [
                Expanded(child: Text(_discovery ? '模型 ID（可选，补充自动获取不到的）' : '模型 ID', style: small)),
                OutlinedButton(onPressed: _fetching || _baseUrl.text.trim().isEmpty ? null : _fetch, child: Text(_fetching ? '获取中…' : '获取模型')),
              ]),
            ),
            if (_fetchError.isNotEmpty) Padding(padding: const EdgeInsets.only(bottom: 4), child: Text(_fetchError, style: TextStyle(fontSize: 12, color: scheme.error))),
            if (fetched.isNotEmpty)
              Container(
                margin: const EdgeInsets.only(bottom: 6),
                decoration: BoxDecoration(border: Border.all(color: scheme.border), borderRadius: BorderRadius.circular(radius)),
                child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                  TextField(
                    controller: _search,
                    style: const TextStyle(fontSize: 12),
                    decoration: InputDecoration(
                      hintText: '搜索 ${fetched.length} 个模型',
                      border: InputBorder.none,
                      enabledBorder: InputBorder.none,
                      focusedBorder: InputBorder.none,
                      contentPadding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
                    ),
                  ),
                  Divider(height: 1, color: scheme.border),
                  ConstrainedBox(
                    constraints: const BoxConstraints(maxHeight: 192),
                    child: ListView(shrinkWrap: true, padding: const EdgeInsets.all(4), children: [
                      for (final id in shown)
                        InkWell(
                          borderRadius: BorderRadius.circular(6),
                          onTap: () => _toggle(id, !listed.contains(id)),
                          child: Row(children: [
                            Checkbox(value: listed.contains(id), visualDensity: VisualDensity.compact, onChanged: (on) => _toggle(id, on == true)),
                            Expanded(child: Text(id, maxLines: 1, overflow: TextOverflow.ellipsis, style: monoStyle(context).copyWith(fontSize: 12))),
                          ]),
                        ),
                      if (shown.isEmpty) Padding(padding: const EdgeInsets.all(8), child: Text('没有匹配的模型', style: small)),
                    ]),
                  ),
                ]),
              ),
            TextField(
              controller: _models,
              minLines: 3,
              maxLines: 6,
              autocorrect: false,
              style: monoStyle(context).copyWith(fontSize: 12),
              decoration: const InputDecoration(hintText: '一行一个\ngpt-4.1\ngpt-4.1-mini'),
            ),
            Padding(padding: const EdgeInsets.only(top: 12, bottom: 4), child: Text('生成图片模型', style: small)),
            ModelSearchField(
              models: widget.models,
              value: _imageModel,
              placeholder: '跟随「模型分工 → 生成图片」',
              onChanged: (model) => setState(() => _imageModel = model),
            ),
            if (_error.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 8), child: Text(_error, style: TextStyle(fontSize: 13, color: scheme.error))),
          ]),
        ),
      ),
      actions: [
        OutlinedButton(onPressed: () => Navigator.pop(context, false), child: const Text('取消')),
        FilledButton(onPressed: _saving ? null : _save, child: Text(_saving ? '保存中…' : '保存')),
      ],
    );
  }
}
