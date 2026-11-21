import 'package:flutter/material.dart';

import '../api/client.dart';
import '../api/models.dart';
import '../state/ashira_state.dart';
import 'theme.dart';
import 'widgets.dart';

/// Settings → Shell: the shell the bash tool runs commands with, saved in the server's config.yml.
class ShellSettings extends StatefulWidget {
  const ShellSettings({super.key, required this.client, required this.state});
  final AshiraClient client;
  final AshiraState state;

  @override
  State<ShellSettings> createState() => _ShellSettingsState();
}

class _ShellSettingsState extends State<ShellSettings> {
  final _draft = TextEditingController();
  ShellStatus? _status;
  String _error = '';
  bool _saving = false;
  bool _saved = false;

  static const _sources = {'env': '环境变量 ASHIRA_SHELL', 'config': 'config.yml', 'auto': '自动查找'};

  @override
  void initState() {
    super.initState();
    _draft.addListener(() => setState(() {}));
    _load();
  }

  @override
  void dispose() {
    _draft.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    try {
      final status = await widget.client.shell();
      if (!mounted) return;
      setState(() {
        _status = status;
        _draft.text = status.configured ?? '';
      });
    } catch (reason) {
      if (mounted) setState(() => _error = reason is ApiException ? reason.message : '读取失败：$reason');
    }
  }

  /// The server runs `printf` through the shell before saving, so a bad path never lands.
  Future<void> _save(String shell) async {
    setState(() {
      _saving = true;
      _error = '';
      _saved = false;
    });
    try {
      final status = await widget.client.setShell(shell.trim());
      if (!mounted) return;
      setState(() {
        _status = status;
        _draft.text = status.configured ?? '';
        _saved = true;
      });
    } catch (reason) {
      if (mounted) setState(() => _error = reason is ApiException ? reason.message : '保存失败：$reason');
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final small = TextStyle(fontSize: 12, color: scheme.mutedForeground);
    final status = _status;
    final changed = _draft.text.trim() != (status?.configured ?? '');
    return Bordered(children: [
      Padding(
        padding: const EdgeInsets.all(12),
        child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
          const Text('Shell', style: TextStyle(fontSize: 14)),
          const SizedBox(height: 2),
          Text('AI 执行命令（bash 工具）用的 shell，需要是 bash、zsh、sh 这类 POSIX shell', style: small),
          if (status != null) ...[
            const SizedBox(height: 10),
            Wrap(spacing: 8, runSpacing: 4, crossAxisAlignment: WrapCrossAlignment.center, children: [
              Text('正在使用', style: small),
              SelectableText(status.current, style: const TextStyle(fontSize: 12, fontFamily: 'monospace')),
              StatusTag(_sources[status.source] ?? status.source),
            ]),
            if (status.envOverride != null) ...[
              const SizedBox(height: 6),
              Text('服务端设置了环境变量 ASHIRA_SHELL，它优先于这里保存的路径。', style: small),
            ],
          ],
          const SizedBox(height: 10),
          TextField(
            controller: _draft,
            autocorrect: false,
            enableSuggestions: false,
            style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
            decoration: const InputDecoration(hintText: r'留空则自动查找，例如 C:\Program Files\Git\bin\bash.exe'),
            onSubmitted: changed && !_saving ? _save : null,
          ),
          const SizedBox(height: 8),
          Wrap(alignment: WrapAlignment.end, spacing: 8, runSpacing: 8, children: [
            if (status?.configured != null) OutlinedButton(onPressed: _saving ? null : () => _save(''), child: const Text('恢复自动')),
            FilledButton(onPressed: _saving || !changed ? null : () => _save(_draft.text), child: Text(_saving ? '测试中…' : '测试并保存')),
          ]),
          if (_error.isNotEmpty) ...[
            const SizedBox(height: 8),
            Text(_error, style: TextStyle(fontSize: 13, color: scheme.error)),
          ] else if (status?.problem != null) ...[
            const SizedBox(height: 8),
            Text(status!.problem!, style: TextStyle(fontSize: 13, color: scheme.error)),
          ],
          if (status?.restartNeeded ?? false) ...[
            const SizedBox(height: 8),
            ListenableBuilder(
              listenable: widget.state,
              builder: (context, _) => Row(children: [
                Expanded(child: Text('${_saved ? '已保存，' : ''}重启服务端后生效', style: small)),
                FilledButton(
                  onPressed: widget.state.restarting ? null : () async {
                    await widget.state.restartServer();
                    await _load();
                  },
                  child: Text(widget.state.restarting ? '重启中…' : '重启'),
                ),
              ]),
            ),
          ] else if (_saved) ...[
            const SizedBox(height: 8),
            Text('已保存', style: small),
          ],
        ]),
      ),
    ]);
  }
}
