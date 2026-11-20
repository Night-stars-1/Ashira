import 'package:flutter/material.dart';

import '../api/models.dart';
import 'format.dart';
import 'theme.dart';
import 'widgets.dart';
import 'window_frame.dart';

/// What a session or folder menu asked for.
enum SessionAction { rename, archive, alias, archiveFolder }

/// Running/asking/naming markers for the whole list, like the web sidebar.
class SessionMarks {
  const SessionMarks({this.running = '', this.asking = false, this.naming = const {}});
  final String running;
  final bool asking;
  final Set<String> naming;
}

class SessionList extends StatelessWidget {
  const SessionList({
    super.key,
    required this.tree,
    required this.activeId,
    required this.marks,
    required this.connected,
    required this.onSelect,
    required this.onCreate,
    required this.onSettings,
    required this.onAction,
  });

  final List<SessionNode> tree;
  final String activeId;
  final SessionMarks marks;
  final bool connected;
  final void Function(String id) onSelect;
  final VoidCallback onCreate, onSettings;

  /// `key` is a session id, or a folder path for folder actions; `label` is its current name.
  final void Function(SessionAction action, String key, String label) onAction;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return SafeArea(
      right: false,
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        WindowDrag(
          child: SizedBox(
            height: 48,
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12),
              child: Row(children: [
                const Expanded(child: Text('Ashira', style: TextStyle(fontSize: 14, fontWeight: FontWeight.w500))),
                Tooltip(
                  message: connected ? '已连接' : '未连接',
                  child: Container(width: 8, height: 8, decoration: BoxDecoration(shape: BoxShape.circle, color: connected ? success : scheme.error)),
                ),
              ]),
            ),
          ),
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(8, 0, 8, 8),
          child: OutlinedButton.icon(
            onPressed: onCreate,
            icon: const Icon(Icons.add),
            label: const Text('新会话'),
            style: OutlinedButton.styleFrom(alignment: Alignment.centerLeft),
          ),
        ),
        Expanded(
          child: tree.isEmpty
              ? Padding(padding: const EdgeInsets.all(16), child: Text('还没有会话', style: TextStyle(fontSize: 13, color: scheme.mutedForeground)))
              : ListView(padding: const EdgeInsets.fromLTRB(8, 0, 8, 8), children: [
                  for (final node in tree) _Project(node: node, depth: 0, activeId: activeId, marks: marks, onSelect: onSelect, onAction: onAction),
                ]),
        ),
        Container(
          padding: const EdgeInsets.all(8),
          decoration: BoxDecoration(border: Border(top: BorderSide(color: scheme.border))),
          child: TextButton.icon(
            onPressed: onSettings,
            icon: const Icon(Icons.settings_outlined),
            label: const Text('设置', style: TextStyle(fontSize: 14)),
            style: TextButton.styleFrom(alignment: Alignment.centerLeft, minimumSize: const Size(0, 36)),
          ),
        ),
      ]),
    );
  }
}

class _Project extends StatefulWidget {
  const _Project({required this.node, required this.depth, required this.activeId, required this.marks, required this.onSelect, required this.onAction});
  final SessionNode node;
  final int depth;
  final String activeId;
  final SessionMarks marks;
  final void Function(String id) onSelect;
  final void Function(SessionAction action, String key, String label) onAction;

  @override
  State<_Project> createState() => _ProjectState();
}

class _ProjectState extends State<_Project> {
  bool _open = true;
  bool _showEmpty = false;

  @override
  Widget build(BuildContext context) {
    final node = widget.node;
    final scheme = Theme.of(context).colorScheme;
    final visible = _showEmpty ? node.sessions : node.sessions.where((item) => item.prompt.isNotEmpty || item.id == widget.activeId).toList();
    final hidden = node.sessions.length - visible.length;
    final indent = 8.0 + widget.depth * 12;
    final small = TextStyle(fontSize: 12, color: scheme.mutedForeground);
    final folderMenu = [
      (SessionAction.alias, '设置别名', Icons.edit_outlined),
      (SessionAction.archiveFolder, '归档文件夹', Icons.archive_outlined),
    ];
    return Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        _MenuRow(
          items: folderMenu,
          onSelected: (action) => widget.onAction(action, node.path, node.name),
          menuLabel: '${node.name} 的更多操作',
          builder: (openMenu, _) => CompactRow(
            onTap: () => setState(() => _open = !_open),
            onLongPress: openMenu,
            padding: EdgeInsets.only(left: indent, right: 32),
            child: Row(children: [
              AnimatedRotation(turns: _open ? .25 : 0, duration: const Duration(milliseconds: 150), child: Icon(Icons.chevron_right, size: 14, color: scheme.mutedForeground)),
              const SizedBox(width: 6),
              Icon(Icons.folder_outlined, size: 14, color: scheme.mutedForeground),
              const SizedBox(width: 6),
              Expanded(child: Text(node.name, maxLines: 1, overflow: TextOverflow.ellipsis, style: TextStyle(fontSize: 12.8, color: scheme.mutedForeground))),
            ]),
          ),
        ),
        if (_open) ...[
          for (final item in visible)
            Padding(
              padding: const EdgeInsets.only(top: 2),
              child: _MenuRow(
                items: const [(SessionAction.rename, '重命名', Icons.edit_outlined), (SessionAction.archive, '归档', Icons.archive_outlined)],
                onSelected: (action) => widget.onAction(action, item.id, item.prompt),
                menuLabel: '${item.prompt.isEmpty ? '空会话' : item.prompt} 的更多操作',
                builder: (openMenu, reveal) => Tooltip(
                  message: item.modified > 0 ? fullTime(item.modified) : '',
                  waitDuration: const Duration(milliseconds: 800),
                  child: GestureDetector(
                    // Double click renames, like the web list.
                    onDoubleTap: () => widget.onAction(SessionAction.rename, item.id, item.prompt),
                    child: CompactRow(
                      selected: item.id == widget.activeId,
                      onTap: () => widget.onSelect(item.id),
                      onLongPress: openMenu,
                      // Room for the 「…」 button once it shows, like the web's `group-hover/row:pr-8`.
                      padding: EdgeInsets.only(left: indent + 8, right: reveal ? 32 : 8),
                      child: Row(children: [
                        // Fixed slot so titles stay aligned whatever the state.
                        SizedBox.square(dimension: 14, child: Center(child: _StatusIcon(item: item, activeId: widget.activeId, marks: widget.marks))),
                        const SizedBox(width: 6),
                        Expanded(
                          child: Text(
                            item.prompt.isEmpty ? '空会话' : item.prompt,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(fontSize: 12.8, color: item.prompt.isEmpty ? scheme.mutedForeground : null),
                          ),
                        ),
                        // The web swaps the time for the 「…」 button on hover.
                        if (!reveal) ...[
                          const SizedBox(width: 8),
                          if (item.id == widget.marks.running)
                            Text(widget.marks.asking ? '待回答' : '进行中', style: TextStyle(fontSize: 11, color: widget.marks.asking ? warning : success))
                          else if (item.modified > 0)
                            Text(ago(item.modified), style: small),
                        ],
                      ]),
                    ),
                  ),
                ),
              ),
            ),
          if (hidden > 0 || _showEmpty)
            CompactRow(
              onTap: () => setState(() => _showEmpty = !_showEmpty),
              padding: EdgeInsets.only(left: indent + 28, right: 8),
              child: Align(alignment: Alignment.centerLeft, child: Text(_showEmpty ? '收起空会话' : '显示 $hidden 个空会话', style: small)),
            ),
          for (final child in node.children)
            _Project(node: child, depth: widget.depth + 1, activeId: widget.activeId, marks: widget.marks, onSelect: widget.onSelect, onAction: widget.onAction),
        ],
      ]),
    );
  }
}

/// The web's status slot: question mark while waiting on an answer, a spinner while
/// running or naming, a dot otherwise (stronger for the open session).
class _StatusIcon extends StatelessWidget {
  const _StatusIcon({required this.item, required this.activeId, required this.marks});
  final SessionItem item;
  final String activeId;
  final SessionMarks marks;
  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final running = item.id == marks.running;
    if (running && marks.asking) return const Tooltip(message: '等待回答', child: Icon(Icons.help_outline, size: 14, color: warning));
    if (running || marks.naming.contains(item.id)) {
      return Tooltip(
        message: running ? '进行中' : '正在生成标题',
        child: SizedBox.square(dimension: 12, child: CircularProgressIndicator(strokeWidth: 1.5, color: running ? success : scheme.mutedForeground)),
      );
    }
    final active = item.id == activeId;
    return Container(
      width: 6,
      height: 6,
      decoration: BoxDecoration(shape: BoxShape.circle, color: active ? scheme.onSurface.withValues(alpha: .7) : scheme.mutedForeground.withValues(alpha: .3)),
    );
  }
}

/// A row with a hover-revealed 「…」 button; right click and long press open the same menu.
/// Like the web, phones always show the button, and [builder] learns when it shows.
class _MenuRow extends StatefulWidget {
  const _MenuRow({required this.items, required this.onSelected, required this.builder, required this.menuLabel});
  final List<(SessionAction, String, IconData)> items;
  final void Function(SessionAction action) onSelected;
  final String menuLabel;
  final Widget Function(VoidCallback openMenu, bool reveal) builder;
  @override
  State<_MenuRow> createState() => _MenuRowState();
}

class _MenuRowState extends State<_MenuRow> {
  final _menu = MenuController();
  bool _hovered = false;

  void _open() => _menu.isOpen ? _menu.close() : _menu.open();

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return MenuAnchor(
      controller: _menu,
      // Opens under the row's start, like the web's dropdown; the platform keeps it on screen.
      alignmentOffset: const Offset(0, 2),
      menuChildren: [
        for (final (action, label, icon) in widget.items)
          MenuItemButton(leadingIcon: Icon(icon, size: 16), onPressed: () => widget.onSelected(action), child: Text(label, style: const TextStyle(fontSize: 13))),
      ],
      builder: (context, controller, _) {
        final reveal = !isDesktop || _hovered || controller.isOpen;
        return MouseRegion(
          onEnter: (_) => setState(() => _hovered = true),
          onExit: (_) => setState(() => _hovered = false),
          child: Stack(alignment: Alignment.centerRight, children: [
            widget.builder(_open, reveal),
            Padding(
              padding: const EdgeInsets.only(right: 4),
              // The web's ghost `icon-xs` button fading in (`transition-all`, 150ms).
              child: IgnorePointer(
                ignoring: !reveal,
                child: AnimatedOpacity(
                  opacity: reveal ? 1 : 0,
                  duration: const Duration(milliseconds: 150),
                  child: SizedBox.square(
                    dimension: 24,
                    child: IconButton(
                      tooltip: widget.menuLabel,
                      padding: EdgeInsets.zero,
                      style: IconButton.styleFrom(minimumSize: const Size.square(24), fixedSize: const Size.square(24), shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(radius))),
                      icon: Icon(Icons.more_horiz, size: 16, color: scheme.onSurface),
                      onPressed: _open,
                    ),
                  ),
                ),
              ),
            ),
          ]),
        );
      },
    );
  }
}
