import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:re_editor/re_editor.dart';

import '../api/client.dart';
import '../api/models.dart';
import 'code_editor.dart';
import 'dialogs.dart';
import 'theme.dart';
import 'widgets.dart';
import 'window_frame.dart';

/// Workspace browser for one session, like the web's FilesSidebar + FileTree:
/// the header holds the current root; folders expand in place and load lazily.
class FilesPanel extends StatefulWidget {
  const FilesPanel({super.key, required this.client, required this.sessionId, required this.cwd, required this.onOpen, this.active = '', this.windowButtons = false});
  final AshiraClient client;
  final String sessionId, cwd;
  final String active;

  /// True when this panel is the top-right corner of the window.
  final bool windowButtons;
  final void Function(String path) onOpen;

  @override
  State<FilesPanel> createState() => _FilesPanelState();
}

class _FilesPanelState extends State<FilesPanel> {
  /// Root of the tree, relative to the session cwd.
  String _root = '';
  final Map<String, List<FileEntry>> _listings = {};
  final Set<String> _open = {};
  final Set<String> _loading = {};
  bool _refreshing = false;
  /// Desktop context menu: [_menuDir] is the folder its 「新建」 items create in;
  /// [_hitDir] is the one the right click landed on, before the menu opens.
  final _menu = MenuController();
  String _menuDir = '';
  String? _hitDir;
  final Map<String, String> _failed = {};

  @override
  void initState() {
    super.initState();
    _load('');
  }

  Future<void> _load(String path) async {
    setState(() {
      _loading.add(path);
      _failed.remove(path);
    });
    try {
      final listing = await widget.client.files(widget.sessionId, path);
      if (mounted) setState(() => _listings[path] = listing.entries);
    } catch (error) {
      if (mounted) setState(() => _failed[path] = errorText(error, '无法读取目录'));
    } finally {
      if (mounted) setState(() => _loading.remove(path));
    }
  }

  void _toggle(String path) {
    setState(() => _open.contains(path) ? _open.remove(path) : _open.add(path));
    if (_open.contains(path) && !_listings.containsKey(path)) _load(path);
  }

  void _setRoot(String path) {
    setState(() => _root = path);
    if (!_listings.containsKey(path)) _load(path);
  }

  static String _join(String parent, String name) => parent.isEmpty ? name : '$parent/$name';
  static String _parentOf(String path) => path.contains('/') ? path.substring(0, path.lastIndexOf('/')) : '';

  /// Creates an entry under [parent] (the root by default) and shows it.
  Future<void> _create({required bool directory, String? parent}) async {
    final base = parent ?? _root;
    final name = await askText(
      context,
      title: directory ? '新建文件夹' : '新建文件',
      description: base == _root ? '相对当前目录的路径' : '相对 ${base.substring(_root.isEmpty ? 0 : _root.length + 1)}/ 的路径',
      hint: directory ? '例如 docs' : '例如 notes.md',
      action: '创建',
    );
    final entered = name?.trim().replaceAll('\\', '/') ?? '';
    if (entered.isEmpty) return;
    final path = _join(base, entered);
    try {
      await widget.client.createEntry(widget.sessionId, path, directory: directory);
      // A folder right-clicked while collapsed opens to show what was made in it.
      if (base != _root) setState(() => _open.add(base));
      await _load(_parentOf(path));
    } catch (error) {
      if (mounted) setState(() => _failed[base] = errorText(error, '新建失败'));
    }
  }

  /// A right click: on a folder it creates there, on a file beside it, elsewhere in the root.
  /// Pointer events, not a tap recognizer: the rows' InkWells would win the gesture arena.
  Offset? _rightDown;
  void _pointerDown(PointerDownEvent event) {
    if (event.kind == PointerDeviceKind.mouse && event.buttons == kSecondaryMouseButton) _rightDown = event.localPosition;
  }

  void _pointerUp(PointerUpEvent event) {
    final down = _rightDown;
    _rightDown = null;
    final dir = _hitDir;
    _hitDir = null;
    if (down == null || (event.localPosition - down).distance > kTouchSlop) return;
    _menuDir = dir ?? _root;
    _menu.open(position: event.localPosition);
  }

  /// Reloads the shown folder and every expanded one in place; a collapsed folder
  /// read earlier is dropped and reloads when it is opened again.
  Future<void> _refresh() async {
    if (_refreshing) return;
    setState(() {
      _refreshing = true;
      _listings.removeWhere((path, _) => path != _root && !_open.contains(path));
    });
    try {
      await Future.wait([_load(_root), for (final path in _open) if (_listings.containsKey(path)) _load(path)]);
    } finally {
      if (mounted) setState(() => _refreshing = false);
    }
  }

  /// Flattens the visible tree so the list virtualizes.
  List<Widget> _rows(String path, int depth, Color muted) {
    final rows = <Widget>[];
    final failure = _failed[path];
    if (failure != null) rows.add(_note(failure, depth, Theme.of(context).colorScheme.error));
    final entries = _listings[path];
    if (entries == null) {
      if (_loading.contains(path)) rows.add(_note('读取中', depth, muted));
      return rows;
    }
    for (final entry in entries) {
      final child = _join(path, entry.name);
      final open = _open.contains(child);
      final row = _TreeRow(
        // Keyed by path so expanding a folder keeps each row's state (the chevron
        // rotation) instead of reusing the widget that happened to sit at that index.
        key: ValueKey(child),
        name: entry.name,
        depth: depth,
        folder: entry.dir,
        open: open,
        active: !entry.dir && child == widget.active,
        onTap: () => entry.dir ? _toggle(child) : widget.onOpen(child),
      );
      // Notes which row a right click hit; the list's handler then opens the menu.
      rows.add(isDesktop
          ? Listener(
              key: ValueKey(child),
              onPointerDown: (event) {
                if (event.buttons & kSecondaryMouseButton != 0) _hitDir = entry.dir ? child : path;
              },
              child: row,
            )
          : row);
      if (entry.dir && open) rows.addAll(_rows(child, depth + 1, muted));
    }
    return rows;
  }

  Widget _note(String text, int depth, Color color) =>
      Padding(padding: EdgeInsets.fromLTRB(8.0 + depth * 12 + 18, 6, 8, 6), child: Text(text, style: TextStyle(fontSize: 12, color: color)));

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final muted = scheme.mutedForeground;
    final location = _root.isEmpty ? widget.cwd : '${widget.cwd.replaceFirst(RegExp(r'[\\/]+$'), '')}/$_root';
    final root = widget.cwd.split(RegExp(r'[\\/]')).lastWhere((part) => part.isNotEmpty, orElse: () => widget.cwd);
    final label = _root.isEmpty ? root : '$root/$_root';
    final rows = _rows(_root, 0, muted);
    return SafeArea(
      left: false,
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        WindowDrag(
          child: SizedBox(
            height: 48,
            child: Row(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
              const SizedBox(width: 8),
              Center(child: _SmallIcon(tooltip: '返回上级目录', icon: Icons.arrow_upward, onPressed: _root.isEmpty ? null : () => _setRoot(_parentOf(_root)))),
              const SizedBox(width: 4),
              Expanded(
                child: Align(
                  alignment: Alignment.centerLeft,
                  child: Tooltip(
                    message: location,
                    child: Container(
                      height: 28,
                      width: double.infinity,
                      padding: const EdgeInsets.symmetric(horizontal: 8),
                      alignment: Alignment.centerLeft,
                      decoration: BoxDecoration(color: scheme.surface.withValues(alpha: .6), border: Border.all(color: scheme.border), borderRadius: BorderRadius.circular(6)),
                      child: Text(label, maxLines: 1, overflow: TextOverflow.ellipsis, style: monoStyle(context).copyWith(fontSize: 12)),
                    ),
                  ),
                ),
              ),
              const SizedBox(width: 4),
              Center(child: _SmallIcon(tooltip: '刷新', icon: Icons.refresh, onPressed: _refreshing ? null : _refresh)),
              // Desktop creates from the right-click menu instead.
              if (!isDesktop) ...[
                Center(child: _SmallIcon(tooltip: '新建文件', icon: Icons.note_add_outlined, onPressed: () => _create(directory: false))),
                Center(child: _SmallIcon(tooltip: '新建文件夹', icon: Icons.create_new_folder_outlined, onPressed: () => _create(directory: true))),
              ],
              if (widget.windowButtons) ...[const SizedBox(width: 4), const WindowButtons()] else const SizedBox(width: 8),
            ]),
          ),
        ),
        Expanded(
          child: RefreshIndicator(
            onRefresh: _refresh,
            child: MenuAnchor(
              controller: _menu,
              menuChildren: [
                MenuItemButton(leadingIcon: const Icon(Icons.note_add_outlined, size: 16), onPressed: () => _create(directory: false, parent: _menuDir), child: const Text('新建文件', style: TextStyle(fontSize: 13))),
                MenuItemButton(leadingIcon: const Icon(Icons.create_new_folder_outlined, size: 16), onPressed: () => _create(directory: true, parent: _menuDir), child: const Text('新建文件夹', style: TextStyle(fontSize: 13))),
              ],
              child: Listener(
                // Opaque so blank space below the rows takes a right click too.
                behavior: HitTestBehavior.opaque,
                onPointerDown: isDesktop ? _pointerDown : null,
                onPointerUp: isDesktop ? _pointerUp : null,
                child: ListView.builder(
                  padding: const EdgeInsets.fromLTRB(8, 0, 8, 8),
                  itemCount: rows.length,
                  itemBuilder: (_, index) => rows[index],
                ),
              ),
            ),
          ),
        ),
      ]),
    );
  }
}

class _TreeRow extends StatelessWidget {
  const _TreeRow({super.key, required this.name, required this.depth, required this.folder, required this.open, required this.active, required this.onTap});
  final String name;
  final int depth;
  final bool folder, open, active;
  final VoidCallback onTap;
  @override
  Widget build(BuildContext context) {
    final muted = Theme.of(context).colorScheme.mutedForeground;
    return CompactRow(
      selected: active,
      onTap: onTap,
      padding: EdgeInsets.only(left: 8.0 + depth * 12, right: 8),
      child: Row(children: [
        SizedBox(
          width: 18,
          child: folder ? AnimatedRotation(turns: open ? .25 : 0, duration: const Duration(milliseconds: 150), child: Icon(Icons.chevron_right, size: 14, color: muted)) : null,
        ),
        Icon(folder ? Icons.folder_outlined : Icons.description_outlined, size: 14, color: muted),
        const SizedBox(width: 6),
        Expanded(child: Text(name, maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 14))),
      ]),
    );
  }
}

/// 24px icon button, like the web's `size="icon-xs"`.
class _SmallIcon extends StatelessWidget {
  const _SmallIcon({required this.tooltip, required this.icon, this.onPressed});
  final String tooltip;
  final IconData icon;
  final VoidCallback? onPressed;
  @override
  Widget build(BuildContext context) => IconButton(
        tooltip: tooltip,
        onPressed: onPressed,
        icon: Icon(icon, size: 14),
        style: IconButton.styleFrom(minimumSize: const Size.square(26), fixedSize: const Size.square(26)),
      );
}

/// Opens the editor: a large dialog on wide windows like the web's FileEditor,
/// a full-screen page on phones. [onFiles] runs after the phone layout's
/// 「文件列表」 button closes the editor, to show the files drawer instead.
Future<void> showFileEditor(BuildContext context, {required AshiraClient client, required String sessionId, required String cwd, required String path, VoidCallback? onFiles}) async {
  final editor = FileEditor(client: client, sessionId: sessionId, cwd: cwd, path: path);
  final size = MediaQuery.sizeOf(context);
  final bool? showFiles;
  if (size.width < 720 && !desktopWindow) {
    showFiles = await Navigator.of(context).push<bool>(MaterialPageRoute(fullscreenDialog: true, builder: (_) => editor));
  } else {
    showFiles = await showDialog<bool>(
      context: context,
      // Closing goes through the editor so unsaved edits can ask first.
      barrierDismissible: false,
      builder: (_) => Dialog(
        clipBehavior: Clip.antiAlias,
        insetPadding: EdgeInsets.zero,
        // Leaves the app visible around it, like the web dialog over the page.
        child: SizedBox(width: (size.width * .85).clamp(0, 1360), height: (size.height * .8).clamp(0, 960), child: editor),
      ),
    );
  }
  if (showFiles == true) onFiles?.call();
}

/// Code editor with the workspace tree beside it on wide windows, like the
/// web's FileEditor. Prompts before discarding unsaved edits.
class FileEditor extends StatefulWidget {
  const FileEditor({super.key, required this.client, required this.sessionId, required this.cwd, required this.path});
  final AshiraClient client;
  final String sessionId, cwd, path;

  @override
  State<FileEditor> createState() => _FileEditorState();
}

class _FileEditorState extends State<FileEditor> {
  final _code = CodeLineEditingController();
  late String _path = widget.path;
  String _saved = '';
  bool _loading = true;
  bool _writing = false;
  String _error = '';
  String? _kind;
  int _request = 0;
  int _offset = 0;
  int? _nextOffset;
  final _previousOffsets = <int>[];

  bool get _dirty => _kind == 'text' && !_loading && _code.text != _saved;

  @override
  void initState() {
    super.initState();
    _code.addListener(() {
      if (_kind == 'text' && !_loading) setState(() {});
    });
    _read(_path);
  }

  Future<void> _read(String path, {int offset = 0, bool? forward}) async {
    final request = ++_request;
    setState(() {
      if (forward == null) {
        _path = path;
        _kind = null;
        _saved = '';
        _writing = false;
        _offset = 0;
        _nextOffset = null;
        _previousOffsets.clear();
      }
      _loading = true;
      _error = '';
    });
    if (forward == null) {
      _code.text = '';
      _code.clearHistory();
    }
    try {
      final file = await widget.client.readFile(widget.sessionId, path, offset: offset);
      if (!mounted || request != _request) return;
      _code.text = file.kind == 'binary' ? '' : file.text;
      _code.clearHistory();
      setState(() {
        _kind = file.kind;
        // Compare against what the editor holds, so line-ending normalization is not an edit.
        _saved = file.kind == 'text' ? _code.text : '';
        if (file.kind == 'large') {
          if (forward == true) _previousOffsets.add(_offset);
          if (forward == false) _previousOffsets.removeLast();
          _offset = offset;
          _nextOffset = file.nextOffset;
        } else {
          _offset = 0;
          _nextOffset = null;
          _previousOffsets.clear();
        }
        _loading = false;
      });
    } catch (error) {
      if (!mounted || request != _request) return;
      setState(() {
        _loading = false;
        _error = errorText(error, '无法读取文件');
      });
    }
  }

  void _movePage(bool forward) {
    if (_loading || _kind != 'large') return;
    final target = forward ? _nextOffset : (_previousOffsets.isEmpty ? null : _previousOffsets.last);
    if (target != null) _read(_path, offset: target, forward: forward);
  }

  /// Opens another file from the tree; unsaved edits ask first.
  Future<void> _open(String path) async {
    if (path == _path || (_dirty && !await _confirmDiscard()) || !mounted) return;
    await _read(path);
  }

  @override
  void dispose() {
    _code.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    if (!_dirty || _writing) return;
    final request = _request;
    final text = _code.text;
    setState(() {
      _writing = true;
      _error = '';
    });
    String? failure;
    try {
      await widget.client.writeFile(widget.sessionId, _path, text);
    } catch (error) {
      failure = errorText(error, '保存失败');
    }
    if (!mounted || request != _request) return;
    setState(() {
      if (failure == null) _saved = text;
      _error = failure ?? '';
      _writing = false;
    });
  }

  Future<bool> _confirmDiscard() async =>
      await showDialog<bool>(
        context: context,
        builder: (context) => SmallDialog(
          title: '放弃修改？',
          description: '当前文件有未保存的修改，确定关闭？',
          actions: [
            OutlinedButton(onPressed: () => Navigator.pop(context, false), child: const Text('继续编辑')),
            FilledButton(onPressed: () => Navigator.pop(context, true), child: const Text('放弃')),
          ],
        ),
      ) ==
      true;

  /// Unsaved edits ask first; `showFiles` asks the caller to open the files drawer.
  Future<void> _close({bool showFiles = false}) async {
    if (_dirty && !await _confirmDiscard()) return;
    if (mounted) Navigator.of(context).pop(showFiles);
  }

  Widget _editorView(BuildContext context) {
    if (_kind == 'text') return CodeView(key: ValueKey(_path), controller: _code, path: _path);
    if (_kind == 'large') {
      final scheme = Theme.of(context).colorScheme;
      final mono = monoStyle(context);
      return CodeEditor(
        key: ValueKey((_path, _offset)),
        controller: _code,
        readOnly: true,
        wordWrap: true,
        padding: const EdgeInsets.all(12),
        style: CodeEditorStyle(
          fontSize: 13,
          fontHeight: 1.5,
          fontFamily: mono.fontFamily,
          fontFamilyFallback: mono.fontFamilyFallback,
          textColor: scheme.onSurface,
          backgroundColor: scheme.surface,
          selectionColor: scheme.outline.withValues(alpha: .4),
        ),
      );
    }
    if (_loading) return const Center(child: SizedBox.square(dimension: 18, child: CircularProgressIndicator()));
    return const SizedBox.shrink();
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final wide = MediaQuery.sizeOf(context).width >= 720;
    final notice = _error.isNotEmpty ? _error : (_kind == 'binary' ? '此文件不是文本，无法预览。' : '');
    final statusStyle = TextStyle(fontSize: 13, color: scheme.mutedForeground);
    return PopScope(
      canPop: !_dirty,
      onPopInvokedWithResult: (popped, _) async {
        if (popped) return;
        if (await _confirmDiscard() && context.mounted) Navigator.of(context).pop();
      },
      child: Material(
        color: Theme.of(context).dialogTheme.backgroundColor,
        child: SafeArea(
          child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 12, 8, 12),
              child: Row(children: [
                Expanded(child: Text(_path, maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 16, fontWeight: FontWeight.w600))),
                // Phones have no tree beside the editor; this closes it and opens the files drawer, like the web.
                if (!wide) IconButton(tooltip: '文件列表', icon: const Icon(Icons.account_tree_outlined), onPressed: () => _close(showFiles: true)),
                IconButton(tooltip: '关闭', icon: const Icon(Icons.close), onPressed: () => _close()),
              ]),
            ),
            Expanded(
              child: Container(
                decoration: BoxDecoration(border: Border.symmetric(horizontal: BorderSide(color: scheme.border))),
                child: Row(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                  // The tree sits beside the text on wide windows; phones get the whole width for editing.
                  if (wide)
                    Container(
                      width: 288,
                      decoration: BoxDecoration(color: scheme.sidebar, border: Border(right: BorderSide(color: scheme.border))),
                      child: FilesPanel(client: widget.client, sessionId: widget.sessionId, cwd: widget.cwd, active: _path, onOpen: _open),
                    ),
                  Expanded(
                    child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                      if (notice.isNotEmpty)
                        Semantics(
                          liveRegion: true,
                          child: Padding(padding: const EdgeInsets.all(12), child: Text(notice, style: statusStyle)),
                        ),
                      Expanded(child: _editorView(context)),
                    ]),
                  ),
                ]),
              ),
            ),
            if (_kind == 'large')
              Padding(
                padding: const EdgeInsets.all(12),
                child: Row(children: [
                  Expanded(child: Text(_loading ? '正在加载…' : '只读预览 · 第 ${_previousOffsets.length + 1} 段', style: statusStyle)),
                  OutlinedButton(onPressed: _loading || _previousOffsets.isEmpty ? null : () => _movePage(false), child: const Text('上一段')),
                  const SizedBox(width: 8),
                  OutlinedButton(onPressed: _loading || _nextOffset == null ? null : () => _movePage(true), child: const Text('下一段')),
                ]),
              ),
            if (_kind == 'text')
              Padding(
                padding: const EdgeInsets.all(12),
                child: Row(children: [
                  if (_dirty) Text('未保存', style: statusStyle),
                  const Spacer(),
                  FilledButton(onPressed: _dirty && !_writing ? _save : null, child: Text(_writing ? '保存中…' : '保存')),
                ]),
              ),
          ]),
        ),
      ),
    );
  }
}
