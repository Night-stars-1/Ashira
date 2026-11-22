import 'package:flutter/material.dart';

import '../api/models.dart';
import '../state/app_update.dart';
import 'format.dart';
import 'theme.dart';
import 'widgets.dart';
import 'window_frame.dart';

/// What a session or folder menu asked for.
enum SessionAction { rename, archive, alias, archiveFolder }

/// Running/asking/naming markers for the whole list, like the web sidebar.
class SessionMarks {
  const SessionMarks({this.running = const {}, this.asking = const {}, this.naming = const {}});
  final Set<String> running, asking;
  final Set<String> naming;
}

class SessionList extends StatelessWidget {
  const SessionList({
    super.key,
    required this.tree,
    required this.activeId,
    required this.marks,
    required this.connected,
    this.serverUpdate,
    this.restartPending = false,
    this.restarting = false,
    this.onRestart,
    required this.onSelect,
    required this.onCreate,
    required this.onSettings,
    required this.onAbout,
    this.onCollapse,
    required this.onAction,
    required this.onFold,
  });

  final List<SessionNode> tree;
  final String activeId;
  final SessionMarks marks;
  final bool connected;
  /// A newer server release tag (`ashira update` installs it), else null.
  final String? serverUpdate;
  /// `ashira update` replaced the server's program; [onRestart] restarts it into the new build.
  final bool restartPending, restarting;
  final VoidCallback? onRestart;
  final void Function(String id) onSelect;
  final VoidCallback onCreate, onSettings;
  /// Opens settings on 关于, where updates are shown and installed.
  final VoidCallback onAbout;
  /// Folds the sidebar away (wide layouts); null where it is a drawer.
  final VoidCallback? onCollapse;

  /// `key` is a session id, or a folder path for folder actions; `label` is its current name.
  final void Function(SessionAction action, String key, String label) onAction;
  /// Folds or unfolds a project folder; kept on the server for every page and App.
  final void Function(String path, bool folded) onFold;

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
              padding: const EdgeInsets.only(right: 12),
              // As a column or a drawer, this header sits in the window's top-left corner.
              child: Row(crossAxisAlignment: CrossAxisAlignment.center, children: [
                const TrafficLightsSpace(normal: 12),
                // Even leading puts the glyphs on the row's center line, level with the dot and «.
                const Text('Ashira', style: TextStyle(fontSize: 14, fontWeight: FontWeight.w500, height: 1, leadingDistribution: TextLeadingDistribution.even)),
                const SizedBox(width: 8),
                Tooltip(
                  message: connected ? '已连接' : '未连接',
                  child: Container(width: 8, height: 8, decoration: BoxDecoration(shape: BoxShape.circle, color: connected ? success : scheme.error)),
                ),
                const Spacer(),
                if (onCollapse != null)
                  IconButton(
                    tooltip: '收起会话列表',
                    iconSize: 18,
                    visualDensity: VisualDensity.compact,
                    icon: Icon(Icons.keyboard_double_arrow_left, color: scheme.mutedForeground),
                    onPressed: onCollapse,
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
                  for (final node in tree) _Project(node: node, depth: 0, activeId: activeId, marks: marks, onSelect: onSelect, onAction: onAction, onFold: onFold),
                ]),
        ),
        // A restart waiting for `ashira update`'s new build comes first; otherwise one quiet line
        // when the App or the server has a newer release (关于 has the details).
        if (restartPending)
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 0, 8, 4),
            child: Row(children: [
              Container(width: 6, height: 6, decoration: BoxDecoration(shape: BoxShape.circle, color: scheme.primary)),
              const SizedBox(width: 8),
              Expanded(child: Text('服务端已更新，重启后生效', maxLines: 1, overflow: TextOverflow.ellipsis, style: TextStyle(fontSize: 12, color: scheme.mutedForeground))),
              FilledButton.icon(
                onPressed: restarting ? null : onRestart,
                icon: const Icon(Icons.restart_alt, size: 14),
                label: Text(restarting ? '重启中' : '重启'),
                style: FilledButton.styleFrom(minimumSize: const Size(0, 26), padding: const EdgeInsets.symmetric(horizontal: 8), textStyle: const TextStyle(fontSize: 12)),
              ),
            ]),
          )
        else
        ListenableBuilder(
          listenable: appUpdate,
          builder: (context, _) {
            final app = appUpdate.available;
            if (app == null && serverUpdate == null) return const SizedBox.shrink();
            final label = app != null && serverUpdate != null
                ? 'App 和服务端有新版本'
                : app != null
                    ? 'App 有新版本 ${app.tag}'
                    : '服务端有新版本 $serverUpdate';
            return Padding(
              padding: const EdgeInsets.fromLTRB(8, 0, 8, 4),
              child: Material(
                color: Colors.transparent,
                borderRadius: BorderRadius.circular(radius),
                clipBehavior: Clip.antiAlias,
                child: InkWell(
                  onTap: onAbout,
                  child: Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 8),
                    child: Row(children: [
                      Container(width: 6, height: 6, decoration: BoxDecoration(shape: BoxShape.circle, color: scheme.primary)),
                      const SizedBox(width: 8),
                      Expanded(child: Text(label, maxLines: 1, overflow: TextOverflow.ellipsis, style: TextStyle(fontSize: 12, color: scheme.mutedForeground))),
                      Icon(Icons.chevron_right, size: 16, color: scheme.mutedForeground),
                    ]),
                  ),
                ),
              ),
            );
          },
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
  const _Project({required this.node, required this.depth, required this.activeId, required this.marks, required this.onSelect, required this.onAction, required this.onFold});
  final SessionNode node;
  final int depth;
  final String activeId;
  final SessionMarks marks;
  final void Function(String id) onSelect;
  final void Function(SessionAction action, String key, String label) onAction;
  /// Folds (`true`) or unfolds a folder; the server keeps it for every page and App.
  final void Function(String path, bool folded) onFold;

  @override
  State<_Project> createState() => _ProjectState();
}

class _ProjectState extends State<_Project> {
  /// Kept on the server, so a folded project stays folded everywhere and after a restart.
  bool get _open => !widget.node.folded;
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
            onTap: () => widget.onFold(widget.node.path, _open),
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
                          if (widget.marks.running.contains(item.id))
                            Text(widget.marks.asking.contains(item.id) ? '待回答' : '进行中', style: TextStyle(fontSize: 11, color: widget.marks.asking.contains(item.id) ? warning : success))
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
            _Project(node: child, depth: widget.depth + 1, activeId: widget.activeId, marks: widget.marks, onSelect: widget.onSelect, onAction: widget.onAction, onFold: widget.onFold),
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
    final running = marks.running.contains(item.id);
    if (marks.asking.contains(item.id)) return const Tooltip(message: '等待回答', child: Icon(Icons.help_outline, size: 14, color: warning));
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
  /// `openMenu(at)`: `at` is where in the row a long press or right click landed.
  final Widget Function(void Function(Offset at) openMenu, bool reveal) builder;
  @override
  State<_MenuRow> createState() => _MenuRowState();
}

class _MenuRowState extends State<_MenuRow> {
  final _menu = MenuController();
  bool _hovered = false;

  /// Opens at `at` in the row (a long press or right click), or under the 「…」 button.
  void _open([Offset? at]) {
    if (_menu.isOpen) return _menu.close();
    _menu.open(position: at ?? _buttonCorner());
  }

  final _button = GlobalKey();

  /// The 「…」 button's bottom-left corner, relative to the row (the anchor).
  Offset? _buttonCorner() {
    final button = _button.currentContext?.findRenderObject() as RenderBox?;
    final row = context.findRenderObject() as RenderBox?;
    if (button == null || row == null) return null;
    return row.globalToLocal(button.localToGlobal(Offset(0, button.size.height + 2)));
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return MenuAnchor(
      controller: _menu,
      // Opens where it was asked for; the platform keeps it on screen.
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
                    key: _button,
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
