import 'package:flutter/material.dart';

import '../api/client.dart';
import '../api/models.dart';
import '../state/ashira_state.dart';
import 'dialogs.dart';
import 'format.dart';
import 'model_settings.dart';
import 'theme.dart';
import 'usage_settings.dart';
import 'widgets.dart';

Future<void> showSettings(BuildContext context, AshiraState state, {required VoidCallback onDisconnect}) => showPanel(
      context,
      title: '设置',
      width: 860,
      builder: (_) => _Settings(state, onDisconnect: onDisconnect),
    );

const _tabs = [('models', '模型'), ('usage', '用量'), ('plugins', '插件'), ('mcp', 'MCP'), ('memory', '记忆'), ('archive', '归档'), ('connection', '连接')];

class _Settings extends StatefulWidget {
  const _Settings(this.state, {required this.onDisconnect});
  final AshiraState state;
  final VoidCallback onDisconnect;
  @override
  State<_Settings> createState() => _SettingsState();
}

class _SettingsState extends State<_Settings> {
  String _tab = 'models';
  AshiraClient get _client => widget.state.client;
  List<String> _plugins = [];
  String _pluginNote = '';
  List<McpServer> _servers = [];
  String _mcpNote = '';
  bool _mcpFailed = false;
  List<MemoryEntry> _memories = [];
  String _memoryNote = '';
  String _mcpError = '';
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    final client = _client;
    await Future.wait([
      client.plugins().then((text) {
        _plugins = text.startsWith('没有') ? [] : text.split('\n').map((line) => line.trim()).where((line) => line.isNotEmpty).toList();
        _pluginNote = '';
      }).catchError((Object error) {
        _plugins = [];
        _pluginNote = errorText(error, '读取失败');
      }),
      client.mcp().then((report) {
        _servers = report.servers;
        _mcpNote = report.warnings.join('\n');
        _mcpFailed = false;
      }).catchError((Object error) {
        _servers = [];
        _mcpNote = errorText(error, '读取失败');
        _mcpFailed = true;
      }),
      client.memory().then((report) {
        _memories = report.entries;
        _memoryNote = '';
      }).catchError((Object error) {
        _memories = [];
        _memoryNote = errorText(error, '读取失败');
      }),
    ]);
    if (mounted) setState(() => _loading = false);
  }

  static String _statusLabel(String status) => switch (status) {
        'connected' => '已连接',
        'connecting' => '连接中',
        'disabled' => '已关闭',
        _ when status == 'error' || status.startsWith('failed') => '失败',
        _ => status.isEmpty ? '未知' : status,
      };

  Future<void> _editMcp([String? name]) async {
    setState(() => _mcpError = '');
    McpForm? current;
    if (name != null) {
      try {
        current = await _client.mcpConfig(name);
      } catch (error) {
        if (mounted) setState(() => _mcpError = errorText(error, '读取配置失败'));
        return;
      }
    }
    if (!mounted) return;
    final saved = await showDialog<bool>(context: context, builder: (_) => _McpForm(_client, initial: current, editing: name));
    if (saved == true) await _load();
  }

  void _showServer(McpServer server) {
    final scheme = Theme.of(context).colorScheme;
    final small = TextStyle(fontSize: 12, color: scheme.mutedForeground);
    final mono = monoStyle(context).copyWith(fontSize: 12, height: 1.5);
    showPanel(
      context,
      title: server.name,
      description: _statusLabel(server.status),
      width: 560,
      height: 560,
      builder: (_) => ListView(padding: const EdgeInsets.all(20), children: [
        if (server.failure.isNotEmpty)
          Container(
            margin: const EdgeInsets.only(bottom: 16),
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
            decoration: BoxDecoration(color: scheme.error.withValues(alpha: .1), borderRadius: BorderRadius.circular(6)),
            child: SelectableText(server.failure, style: TextStyle(fontSize: 13, color: scheme.error)),
          ),
        Bordered(children: [
          for (final (label, value, code) in [
            if (server.kind.isNotEmpty) ('传输', server.kind, false),
            if (server.target.isNotEmpty) (server.kind == 'stdio' ? '命令' : '地址', server.target, true),
            ('配置', server.source, true),
          ])
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 9),
              child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
                SizedBox(width: 64, child: Text(label, style: TextStyle(fontSize: 13, color: scheme.mutedForeground))),
                Expanded(child: SelectableText(value, style: code ? mono : const TextStyle(fontSize: 13))),
              ]),
            ),
        ]),
        const SizedBox(height: 20),
        SectionTitle('工具（${server.tools.length}）'),
        if (server.tools.isEmpty)
          Text('没有可用工具。', style: small)
        else
          Bordered(children: [
            for (final tool in server.tools)
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                  Text(tool.name, style: mono),
                  if (tool.description.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 2), child: Text(tool.description, style: small)),
                ]),
              ),
          ]),
      ]),
    );
  }

  Future<void> _removeMcp(String name) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => SmallDialog(
        title: '删除 $name？',
        description: '会从 Ashira 的 MCP 配置文件中移除这个服务器并断开连接，无法撤销。',
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
    setState(() => _mcpError = '');
    try {
      await _client.mcpRemove(name);
      // The file is already written; drop the row before the refetch.
      if (mounted) setState(() => _servers = _servers.where((server) => server.name != name).toList());
      await _load();
    } catch (error) {
      if (mounted) setState(() => _mcpError = errorText(error, '删除失败'));
    }
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final narrow = MediaQuery.sizeOf(context).width < 720;
    final small = TextStyle(fontSize: 12, color: scheme.mutedForeground);
    Widget note(String text, {bool error = false}) =>
        Padding(padding: const EdgeInsets.only(bottom: 12), child: Text(text, style: TextStyle(fontSize: 13, color: error ? scheme.error : scheme.mutedForeground)));
    Widget card(Widget child) => Padding(
          padding: const EdgeInsets.only(bottom: 8),
          child: Container(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
            decoration: BoxDecoration(border: Border.all(color: scheme.border), borderRadius: BorderRadius.circular(radius)),
            child: child,
          ),
        );

    // Sessions grouped under their folder name, so two "hi" threads stay distinguishable.
    String folderOf(String cwd) => cwd.split(RegExp(r'[\\/]')).lastWhere((part) => part.isNotEmpty, orElse: () => cwd);
    Widget restoreRow(String title, String subtitle, VoidCallback onRestore, {bool muted = false}) => card(Row(children: [
          Expanded(
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Text(title, maxLines: 1, overflow: TextOverflow.ellipsis, style: TextStyle(fontSize: 13, color: muted ? scheme.mutedForeground : null)),
              Tooltip(message: subtitle, child: Text(subtitle, maxLines: 1, overflow: TextOverflow.ellipsis, style: small)),
            ]),
          ),
          OutlinedButton(onPressed: onRestore, child: const Text('还原')),
        ]));

    final content = switch (_tab) {
      'models' => [ModelSettings(client: _client)],
      'usage' => [UsageSettings(client: _client)],
      'archive' => [
          ListenableBuilder(
            listenable: widget.state,
            builder: (context, _) {
              final state = widget.state;
              if (state.archivedFolders.isEmpty && state.archivedSessions.isEmpty) {
                return const EmptyState(title: '没有归档', description: '在会话列表里用「…」或长按归档会话和文件夹，归档后可在这里还原。');
              }
              return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                if (state.archivedFolders.isNotEmpty) ...[
                  Padding(padding: const EdgeInsets.only(bottom: 6), child: Text('文件夹', style: small)),
                  for (final folder in state.archivedFolders) restoreRow(folder.name, folder.path, () => state.setArchived('folder', folder.path, archived: false)),
                ],
                if (state.archivedSessions.isNotEmpty) ...[
                  Padding(padding: EdgeInsets.only(bottom: 6, top: state.archivedFolders.isEmpty ? 0 : 8), child: Text('会话', style: small)),
                  for (final item in state.archivedSessions)
                    restoreRow(
                      item.prompt.isEmpty ? '空会话' : item.prompt,
                      [folderOf(item.cwd), if (item.modified > 0) fullTime(item.modified).split(' ').first].join(' · '),
                      () => state.setArchived('session', item.id, archived: false),
                      muted: item.prompt.isEmpty,
                    ),
                ],
              ]);
            },
          ),
        ],
      'plugins' => [
          if (_pluginNote.isNotEmpty) note(_pluginNote, error: true) else if (_plugins.isEmpty) const EmptyState(title: '没有插件', description: '把插件放到 ~/.ashira/plugins 或项目的 .ashira/plugins。'),
          for (final line in _plugins) card(Text(line, style: const TextStyle(fontSize: 13))),
        ],
      'mcp' => [
          Align(alignment: Alignment.centerRight, child: Padding(padding: const EdgeInsets.only(bottom: 12), child: FilledButton(onPressed: _editMcp, child: const Text('添加')))),
          if (_mcpError.isNotEmpty) note(_mcpError, error: true),
          if (_mcpNote.isNotEmpty) note(_mcpNote, error: _mcpFailed),
          if (!_mcpFailed && _servers.isEmpty) const EmptyState(title: '没有 MCP', description: '添加一个本地命令或远程地址。写入后会重新连接。'),
          for (final server in _servers)
            Padding(
              padding: const EdgeInsets.only(bottom: 8),
              child: Material(
                color: Colors.transparent,
                shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(radius), side: BorderSide(color: scheme.border)),
                clipBehavior: Clip.antiAlias,
                child: InkWell(
                  onTap: () => _showServer(server),
                  child: Padding(
                    padding: const EdgeInsets.fromLTRB(12, 8, 4, 8),
                    child: Row(children: [
                      Expanded(
                        child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                          Text(server.name, maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w500)),
                          Text([server.kind.isEmpty ? '已关闭' : server.kind, if (server.tools.isNotEmpty) '${server.tools.length} 个工具'].join(' · '), maxLines: 1, style: small),
                        ]),
                      ),
                      StatusTag(_statusLabel(server.status), destructive: server.status.startsWith('failed') || server.status == 'error'),
                      const SizedBox(width: 4),
                      TextButton(onPressed: () => _editMcp(server.name), child: const Text('编辑')),
                      TextButton(
                        style: TextButton.styleFrom(foregroundColor: scheme.error),
                        onPressed: () => _removeMcp(server.name),
                        child: const Text('删除'),
                      ),
                    ]),
                  ),
                ),
              ),
            ),
        ],
      'memory' => [
          if (_memoryNote.isNotEmpty) note(_memoryNote, error: true) else if (_memories.isEmpty) const EmptyState(title: '没有记忆', description: '对话里让 Ashira 记住的内容会出现在这里。'),
          for (final entry in _memories)
            card(Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Text(entry.key, style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w500)),
              const SizedBox(height: 4),
              SelectableText(entry.value, style: TextStyle(fontSize: 13, color: scheme.mutedForeground, height: 1.5)),
            ])),
        ],
      _ => [
          Bordered(children: [
            InfoRow('服务器', _client.baseUrl),
            InfoRow('API Token', _client.token.isEmpty ? '未设置' : '已设置'),
          ]),
          const SizedBox(height: 12),
          Align(
            alignment: Alignment.centerRight,
            child: OutlinedButton.icon(
              icon: const Icon(Icons.logout),
              label: const Text('断开并更换服务器'),
              onPressed: () {
                Navigator.of(context).pop();
                widget.onDisconnect();
              },
            ),
          ),
        ],
    };

    final waiting = _loading && const {'plugins', 'mcp', 'memory'}.contains(_tab);
    final body = waiting
        ? const Center(child: SizedBox.square(dimension: 18, child: CircularProgressIndicator()))
        // Keyed per tab so each tab's own state (model form, usage range) starts fresh.
        : ListView(key: PageStorageKey(_tab), padding: const EdgeInsets.all(16), children: content);

    if (narrow) {
      // Phones: a scrolling row of underlined tabs, like the web.
      return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        DefaultTabController(
          length: _tabs.length,
          initialIndex: _tabs.indexWhere((tab) => tab.$1 == _tab),
          child: TabBar(
            isScrollable: true,
            tabAlignment: TabAlignment.start,
            padding: const EdgeInsets.symmetric(horizontal: 8),
            labelPadding: const EdgeInsets.symmetric(horizontal: 12),
            labelStyle: const TextStyle(fontSize: 14, fontWeight: FontWeight.w500),
            unselectedLabelStyle: const TextStyle(fontSize: 14, fontWeight: FontWeight.w500),
            indicator: UnderlineTabIndicator(borderSide: BorderSide(width: 2, color: scheme.onSurface), borderRadius: BorderRadius.circular(1)),
            indicatorSize: TabBarIndicatorSize.label,
            onTap: (next) => setState(() => _tab = _tabs[next].$1),
            tabs: [for (final (_, label) in _tabs) Tab(height: 40, text: label)],
          ),
        ),
        Expanded(child: body),
      ]);
    }
    return Row(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      Container(
        width: 148,
        padding: const EdgeInsets.all(8),
        decoration: BoxDecoration(border: Border(right: BorderSide(color: scheme.border))),
        child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
          for (final (id, label) in _tabs)
            Padding(
              padding: const EdgeInsets.only(bottom: 4),
              child: CompactRow(
                selected: _tab == id,
                onTap: () => setState(() => _tab = id),
                padding: const EdgeInsets.symmetric(horizontal: 10),
                child: Align(alignment: Alignment.centerLeft, child: Text(label, style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w500))),
              ),
            ),
        ]),
      ),
      Expanded(child: body),
    ]);
  }
}

class _McpForm extends StatefulWidget {
  const _McpForm(this.client, {this.initial, this.editing});
  final AshiraClient client;
  final McpForm? initial;

  /// Name of the server being edited; null when adding.
  final String? editing;
  @override
  State<_McpForm> createState() => _McpFormState();
}

class _McpFormState extends State<_McpForm> {
  late final _name = TextEditingController(text: widget.initial?.name);
  late final _command = TextEditingController(text: widget.initial?.command);
  late final _args = TextEditingController(text: widget.initial?.args);
  late final _url = TextEditingController(text: widget.initial?.url);
  late final _headers = TextEditingController(text: widget.initial?.headers);
  late String _type = widget.initial?.type ?? 'stdio';
  String _error = '';
  bool _saving = false;

  @override
  void dispose() {
    for (final controller in [_name, _command, _args, _url, _headers]) {
      controller.dispose();
    }
    super.dispose();
  }

  Future<void> _save() async {
    setState(() {
      _saving = true;
      _error = '';
    });
    try {
      final form = McpForm(
        name: _name.text.trim(),
        type: _type,
        command: _command.text.trim(),
        args: _args.text.trim(),
        url: _url.text.trim(),
        headers: _headers.text.trim(),
      );
      final editing = widget.editing;
      if (editing == null) {
        await widget.client.mcpAdd(form);
      } else {
        await widget.client.mcpUpdate(editing, form);
      }
      if (mounted) Navigator.pop(context, true);
    } catch (error) {
      if (mounted) setState(() => _error = errorText(error, '保存失败'));
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) => SmallDialog(
        title: widget.editing == null ? '添加 MCP' : '编辑 ${widget.editing}',
        description: '本地命令用 stdio，远程服务用 http 或 sse。保存后会重新连接。',
        body: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.stretch, children: [
          TextField(controller: _name, decoration: const InputDecoration(hintText: '名称'), onChanged: (_) => setState(() {})),
          const SizedBox(height: 10),
          Wrap(spacing: 4, children: [
            for (final type in const ['stdio', 'http', 'sse']) Pill(label: type, selected: _type == type, onTap: () => setState(() => _type = type)),
          ]),
          const SizedBox(height: 10),
          if (_type == 'stdio') ...[
            TextField(controller: _command, decoration: const InputDecoration(hintText: '命令，例如 npx')),
            const SizedBox(height: 10),
            TextField(controller: _args, decoration: const InputDecoration(hintText: '参数，用空格分开')),
          ] else ...[
            TextField(controller: _url, decoration: const InputDecoration(hintText: 'https://example.com/mcp')),
            const SizedBox(height: 10),
            TextField(
              controller: _headers,
              minLines: 3,
              maxLines: 6,
              style: monoStyle(context).copyWith(fontSize: 12),
              decoration: const InputDecoration(hintText: '请求头，可选。一行一个\nAuthorization: Bearer token\nX-Api-Key: \${API_KEY}'),
            ),
          ],
          if (_error.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 10), child: Text(_error, style: TextStyle(fontSize: 13, color: Theme.of(context).colorScheme.error))),
        ]),
        actions: [
          OutlinedButton(onPressed: () => Navigator.pop(context), child: const Text('取消')),
          FilledButton(onPressed: _saving || _name.text.trim().isEmpty ? null : _save, child: Text(widget.editing == null ? '添加' : '保存')),
        ],
      );
}
