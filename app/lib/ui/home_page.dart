import 'package:flutter/material.dart';

import '../state/ashira_state.dart';
import 'composer.dart';
import 'dialogs.dart';
import 'files.dart';
import 'session_usage.dart';
import 'sessions.dart';
import 'settings_view.dart';
import 'theme.dart';
import 'widgets.dart';
import 'terminal_view.dart';
import 'transcript.dart';
import 'workspace_image.dart';
import 'window_frame.dart';

class HomePage extends StatefulWidget {
  const HomePage({super.key, required this.state, required this.onDisconnect});
  final AshiraState state;
  final VoidCallback onDisconnect;

  @override
  State<HomePage> createState() => _HomePageState();
}

class _HomePageState extends State<HomePage> {
  final _scaffold = GlobalKey<ScaffoldState>();
  int? _shownConfirm;
  bool _filesOpen = true;
  bool _sessionsOpen = true;
  String _activeFile = '';

  /// 对话 or 终端. The terminal mounts on first use and then stays alive, like the web's
  /// `terminalOpened` with `unmount-on-hide="false"`; a session switch resets both.
  bool _terminal = false;
  bool _terminalOpened = false;
  String _viewSession = '';

  AshiraState get state => widget.state;

  @override
  void initState() {
    super.initState();
    state.addListener(_onState);
  }

  @override
  void dispose() {
    state.removeListener(_onState);
    super.dispose();
  }

  void _onState() {
    final request = state.confirm;
    if (request != null && request.id != _shownConfirm) {
      _shownConfirm = request.id;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) showConfirm(context, state, request);
      });
    }
    if (state.unauthorized) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) widget.onDisconnect();
      });
    }
  }

  Future<void> _create() async {
    _scaffold.currentState?.closeDrawer();
    final cwd = await showProjectPicker(context, state);
    if (cwd != null) await state.openSession(create: true, cwd: cwd);
  }

  void _select(String id) {
    _scaffold.currentState?.closeDrawer();
    if (id != state.shownId) state.openSession(id: id);
  }

  Future<void> _action(SessionAction action, String key, String label) async {
    switch (action) {
      case SessionAction.alias:
        final alias = await askText(context, title: '设置别名', description: key, initial: label, hint: '留空则恢复目录名');
        if (alias == null) return;
        try {
          await state.client.alias(key, alias.trim());
          await state.refresh();
        } catch (error) {
          if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(errorText(error, '别名保存失败'))));
        }
      case SessionAction.rename:
        final title = await askText(context, title: '重命名会话', description: '留空则恢复为 AI 生成的标题。', initial: label, hint: '会话标题');
        if (title != null) await state.rename(key, title);
      case SessionAction.archive:
        await state.setArchived('session', key, archived: true);
      case SessionAction.archiveFolder:
        await state.setArchived('folder', key, archived: true);
    }
  }

  Future<void> _openFile(String path) async {
    _scaffold.currentState?.closeEndDrawer();
    setState(() => _activeFile = path);
    await showFileEditor(
      context,
      client: state.client,
      sessionId: state.shownId,
      cwd: state.cwd,
      path: path,
      onFiles: () => _scaffold.currentState?.openEndDrawer(),
    );
  }

  Widget _sessions({bool collapsible = false}) => SessionList(
        tree: state.tree,
        activeId: state.shownId,
        marks: SessionMarks(running: state.running, asking: state.asking, naming: state.naming),
        connected: state.connected,
        serverUpdate: state.serverUpdate,
        onSelect: _select,
        onCreate: _create,
        onSettings: () {
          _scaffold.currentState?.closeDrawer();
          showSettings(context, state, onDisconnect: widget.onDisconnect);
        },
        onAbout: () {
          _scaffold.currentState?.closeDrawer();
          showSettings(context, state, onDisconnect: widget.onDisconnect, tab: 'about');
        },
        onCollapse: collapsible ? () => setState(() => _sessionsOpen = false) : null,
        onAction: _action,
      );

  Widget? _files({bool windowButtons = false}) => state.shownId.isEmpty
      ? null
      : FilesPanel(
          key: ValueKey(state.shownId),
          client: state.client,
          sessionId: state.shownId,
          cwd: state.cwd,
          active: _activeFile,
          onOpen: _openFile,
          windowButtons: windowButtons,
        );

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: state,
      builder: (context, _) {
        final scheme = Theme.of(context).colorScheme;
        final wide = MediaQuery.sizeOf(context).width >= 1000;
        final hasSession = state.shownId.isNotEmpty;
        if (state.shownId != _viewSession) {
          _viewSession = state.shownId;
          _terminal = false;
          _terminalOpened = false;
        }
        final terminal = _terminal && hasSession;
        final filesShown = wide && _filesOpen && hasSession;
        // The window buttons end whichever header sits in the top-right corner.
        final files = _files(windowButtons: filesShown);
        final header = WindowDrag(
          child: PanelHeader(
            padding: EdgeInsets.only(left: wide ? (_sessionsOpen ? 24 : 12) : 4, right: filesShown || !customFrame ? (wide ? 24 : 4) : 0),
            children: [
              if (!wide || !_sessionsOpen)
                IconButton(
                  tooltip: wide ? '展开会话列表' : '会话列表',
                  icon: Icon(wide ? Icons.keyboard_double_arrow_right : Icons.menu),
                  onPressed: () => wide ? setState(() => _sessionsOpen = true) : _scaffold.currentState?.openDrawer(),
                ),
              if (!wide || !_sessionsOpen) const SizedBox(width: 4),
              if (state.naming.contains(state.shownId))
                Padding(
                  padding: const EdgeInsets.only(right: 8),
                  child: Tooltip(message: '正在生成标题', child: SizedBox.square(dimension: 12, child: CircularProgressIndicator(strokeWidth: 1.5, color: scheme.mutedForeground))),
                ),
              Expanded(child: Text(state.sessionName, maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 14, fontWeight: FontWeight.w500))),
              _ViewTabs(
                terminal: terminal,
                compact: !wide && MediaQuery.sizeOf(context).width < 640,
                terminalEnabled: hasSession,
                onSelect: (next) => setState(() {
                  _terminal = next;
                  if (next) _terminalOpened = true;
                }),
              ),
              const SizedBox(width: 4),
              IconButton(tooltip: '用量', icon: const Icon(Icons.bar_chart_rounded), onPressed: hasSession ? () => showUsage(context, state) : null),
              IconButton(
                tooltip: wide ? (_filesOpen ? '收起文件' : '展开文件') : '文件',
                icon: Icon(wide && _filesOpen ? Icons.view_sidebar : Icons.folder_open_outlined),
                onPressed: !hasSession ? null : () => wide ? setState(() => _filesOpen = !_filesOpen) : _scaffold.currentState?.openEndDrawer(),
              ),
              if (!filesShown && customFrame) ...[const SizedBox(width: 8), const SizedBox(height: 47, child: WindowButtons())],
            ],
          ),
        );
        final conversation = Column(children: [
          Expanded(
            // Opening a session stays blank until its history arrives, like the web.
            child: state.loading
                ? const SizedBox.shrink()
                : state.blocks.isEmpty && state.activity.isEmpty
                ? _Empty(hasSession: hasSession, cwd: state.cwd)
                : WorkspaceImages(
                    uri: (path) => state.client.rawUri(state.shownId, path),
                    child: Transcript(
                      key: ValueKey(state.shownId),
                      blocks: state.blocks,
                      activity: state.activity,
                      onOpenAgent: (id) => showSubagent(context, state.client, id, (path) => state.client.rawUri(state.shownId, path)),
                    ),
                  ),
          ),
          Composer(
            key: ValueKey('composer-${state.shownId}'),
            enabled: hasSession && state.connected,
            hasSession: hasSession,
            busy: state.busy,
            model: state.modelTitle,
            context: state.status?.context ?? 0,
            window: state.status?.window,
            error: state.error,
            onSend: state.send,
            onInterrupt: state.interrupt,
            onModels: () => showModelPicker(context, state),
            question: state.question,
            onAnswer: state.answer,
          ),
        ]);
        final chat = Column(children: [
          header,
          Expanded(
            child: IndexedStack(
              index: terminal ? 1 : 0,
              sizing: StackFit.expand,
              children: [
                conversation,
                if (_terminalOpened && hasSession)
                  SessionTerminal(key: ValueKey('terminal-${state.shownId}'), client: state.client, sessionId: state.shownId, active: terminal)
                else
                  const SizedBox.shrink(),
              ],
            ),
          ),
        ]);
        Widget side(Widget child, {required bool left}) => Container(
              // The right panel also holds the window buttons on custom-frame desktops.
              width: !left && customFrame ? 320 : 264,
              decoration: BoxDecoration(
                color: scheme.sidebar,
                border: Border(left: left ? BorderSide.none : BorderSide(color: scheme.border), right: left ? BorderSide(color: scheme.border) : BorderSide.none),
              ),
              child: child,
            );
        return Scaffold(
          key: _scaffold,
          drawer: wide ? null : Drawer(child: _sessions()),
          endDrawer: wide || files == null ? null : Drawer(child: files),
          body: SafeArea(
            bottom: false,
            child: wide
                ? Row(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                    ClipRect(
                      child: AnimatedAlign(
                        duration: const Duration(milliseconds: 200),
                        curve: Curves.easeOutCubic,
                        alignment: Alignment.centerRight,
                        widthFactor: _sessionsOpen ? 1 : 0,
                        child: side(_sessions(collapsible: true), left: true),
                      ),
                    ),
                    Expanded(child: chat),
                    // Slides open and shut instead of popping; the panel keeps its width
                    // while the clip grows, so its contents do not reflow mid-animation.
                    if (files != null)
                      ClipRect(
                        child: AnimatedAlign(
                          duration: const Duration(milliseconds: 200),
                          curve: Curves.easeOutCubic,
                          alignment: Alignment.centerLeft,
                          widthFactor: filesShown ? 1 : 0,
                          child: side(files, left: false),
                        ),
                      ),
                  ])
                : chat,
          ),
        );
      },
    );
  }
}

class _Empty extends StatelessWidget {
  const _Empty({required this.hasSession, required this.cwd});
  final bool hasSession;
  final String cwd;
  @override
  Widget build(BuildContext context) => Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(mainAxisSize: MainAxisSize.min, children: [
            Text(hasSession ? '这个目录里要做什么？' : '新建一个会话开始', style: Theme.of(context).textTheme.titleLarge?.copyWith(fontWeight: FontWeight.w500)),
            const SizedBox(height: 8),
            Text(cwd, textAlign: TextAlign.center, maxLines: 2, overflow: TextOverflow.ellipsis, style: TextStyle(fontSize: 13, color: Theme.of(context).colorScheme.mutedForeground)),
          ]),
        ),
      );
}

/// The web's 对话/终端 TabsList: a muted track with the selected tab raised on the
/// background with a border. Phones show only the icons, like `max-sm:size-7`.
class _ViewTabs extends StatelessWidget {
  const _ViewTabs({required this.terminal, required this.compact, required this.terminalEnabled, required this.onSelect});
  final bool terminal, compact, terminalEnabled;
  final ValueChanged<bool> onSelect;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    Widget tab(bool value, IconData icon, String label, {bool enabled = true}) {
      final selected = terminal == value;
      final color = selected ? scheme.onSurface : scheme.mutedForeground;
      return Tooltip(
        message: label,
        child: Opacity(
          opacity: enabled ? 1 : .5,
          child: MouseRegion(
            cursor: enabled ? SystemMouseCursors.click : SystemMouseCursors.basic,
            child: GestureDetector(
              onTap: enabled && !selected ? () => onSelect(value) : null,
              child: AnimatedContainer(
                duration: const Duration(milliseconds: 150),
                height: compact ? 28 : 24,
                width: compact ? 28 : null,
                padding: compact ? null : const EdgeInsets.symmetric(horizontal: 8),
                alignment: Alignment.center,
                decoration: BoxDecoration(
                  color: selected ? scheme.surface : Colors.transparent,
                  borderRadius: BorderRadius.circular(5),
                  border: Border.all(color: selected ? scheme.border : Colors.transparent),
                ),
                child: Row(mainAxisSize: MainAxisSize.min, children: [
                  Icon(icon, size: 14, color: color),
                  if (!compact) ...[
                    const SizedBox(width: 4),
                    Text(label, style: TextStyle(fontSize: 12.8, fontWeight: FontWeight.w500, color: color)),
                  ],
                ]),
              ),
            ),
          ),
        ),
      );
    }

    return Container(
      padding: const EdgeInsets.all(2),
      decoration: BoxDecoration(color: scheme.muted, borderRadius: BorderRadius.circular(radius)),
      child: Row(mainAxisSize: MainAxisSize.min, children: [
        tab(false, Icons.chat_bubble_outline, '对话'),
        const SizedBox(width: 2),
        tab(true, Icons.terminal, '终端', enabled: terminalEnabled),
      ]),
    );
  }
}
