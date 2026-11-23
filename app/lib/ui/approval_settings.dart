import 'package:flutter/material.dart';

import '../api/client.dart';
import '../api/models.dart';
import '../state/ashira_state.dart';
import 'theme.dart';
import 'widgets.dart';

/// The approval modes the chat switch and 设置 → 通用 offer.
const approvalOptions = {
  'manual': (label: '手动审批', detail: '危险 Shell 命令和未授权的 MCP 工具需询问。'),
  'auto': (label: '自动审批', detail: '允许低风险、受信任或白名单操作，自动拒绝危险操作和未授权的 MCP 工具。'),
  'full': (label: '完整权限', detail: '跳过工具审批，但不会增加 Ashira 的系统权限。'),
};

/// One menu row per mode, its label over its detail; [onPick] null greys them all out.
List<Widget> approvalMenu(BuildContext context, String? current, void Function(String mode)? onPick) {
  final scheme = Theme.of(context).colorScheme;
  return [
    for (final entry in approvalOptions.entries)
      MenuItemButton(
        onPressed: onPick == null || current == entry.key ? null : () => onPick(entry.key),
        leadingIcon: current == entry.key ? const Icon(Icons.check, size: 16) : const SizedBox.square(dimension: 16),
        child: SizedBox(
          width: 210,
          child: Padding(
            padding: const EdgeInsets.symmetric(vertical: 4),
            child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
              Text(entry.value.label),
              Text(entry.value.detail, style: TextStyle(fontSize: 12, color: scheme.mutedForeground)),
            ]),
          ),
        ),
      ),
  ];
}

/// 设置 → 通用 → 审批: config.yml `approval:` on the server, the mode sessions start in and
/// whether the chat shows its switch. Rows of a [SettingsCard].
class ApprovalSettingsCard extends StatefulWidget {
  const ApprovalSettingsCard({super.key, required this.state});
  final AshiraState state;

  @override
  State<ApprovalSettingsCard> createState() => _ApprovalSettingsCardState();
}

class _ApprovalSettingsCardState extends State<ApprovalSettingsCard> {
  ApprovalSettings? _settings;
  String _error = '';
  bool _saving = false;

  AshiraClient get _client => widget.state.client;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final settings = await _client.approval();
      if (mounted) setState(() => _settings = settings);
    } catch (reason) {
      if (mounted) setState(() => _error = reason is ApiException ? reason.message : '读取失败：$reason');
    }
  }

  Future<void> _save({String? defaultMode, bool? hidden}) async {
    final current = _settings;
    if (current == null || _saving) return;
    setState(() {
      _saving = true;
      _error = '';
    });
    try {
      final saved = await _client.setApproval(defaultMode: defaultMode ?? current.defaultMode, hidden: hidden ?? current.hidden);
      if (!mounted) return;
      setState(() => _settings = saved);
      widget.state.approvalSaved(saved);
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
    final settings = _settings;
    final ready = settings != null && !_saving;
    Widget label(String title, String description) => Expanded(
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text(title, style: const TextStyle(fontSize: 14)),
            const SizedBox(height: 2),
            Text(description, style: small),
          ]),
        );
    return SettingsCard(children: [
      Row(children: [
        label('默认审批模式', '新打开的会话使用此模式'),
        const SizedBox(width: 12),
        MenuAnchor(
          menuChildren: approvalMenu(context, settings?.defaultMode, ready ? (mode) => _save(defaultMode: mode) : null),
          builder: (context, controller, _) => OutlinedButton(
            onPressed: ready ? () => controller.isOpen ? controller.close() : controller.open() : null,
            child: Row(mainAxisSize: MainAxisSize.min, children: [
              Text(settings == null ? '读取中' : approvalOptions[settings.defaultMode]!.label),
              const SizedBox(width: 4),
              const Icon(Icons.expand_more, size: 16),
            ]),
          ),
        ),
      ]),
      Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        Row(children: [
          label('在输入框显示审批按钮', '关闭后所有会话都按默认审批模式运行'),
          Toggle(label: '在输入框显示审批按钮', value: settings == null || !settings.hidden, onChanged: ready ? (shown) => _save(hidden: !shown) : null),
        ]),
        if (settings?.forced ?? false) Padding(padding: const EdgeInsets.only(top: 8), child: Text('服务端以 --yes 启动，所有会话固定为完整权限，默认模式在去掉 --yes 后生效', style: small)),
        if (_error.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 8), child: Text(_error, style: TextStyle(fontSize: 13, color: scheme.error))),
      ]),
    ]);
  }
}
