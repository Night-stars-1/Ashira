import 'dart:async';

import 'package:flutter/foundation.dart';

import '../api/client.dart';
import '../api/models.dart';
import 'app_update.dart';
import 'settings.dart';

const _coordination = {'task', 'wait', 'task_status', 'task_cancel'};

/// Session state for one server, ported from `web/src/lib/session.ts`.
///
/// `_view` invalidates async work when the shown session changes; `_revision`
/// detects SSE frames that arrived while a history fetch was in flight.
class AshiraState extends ChangeNotifier {
  AshiraState(this.settings) : client = AshiraClient(baseUrl: settings.baseUrl, token: settings.token);

  final ConnectionSettings settings;
  final AshiraClient client;

  Status? status;
  /// The server's version, the newer release it can update to, and whether `ashira update`
  /// already replaced its program (a restart loads it); from `GET /v1`.
  String serverVersion = '';
  String? serverUpdate;

  /// The release the server is behind: its own check (cached for a day on the server), else
  /// the newer release the App found. Right after a release the server's check still says none.
  String? get serverBehind {
    if (serverUpdate != null) return serverUpdate;
    final latest = appUpdate.latest;
    return latest != null && serverVersion.isNotEmpty && newer(latest, serverVersion) ? latest : null;
  }
  bool restartPending = false;
  /// The server runs from the Docker image: it is updated by pulling the image.
  bool serverInContainer = false;
  bool restarting = false;
  bool updating = false;
  /// The server update's download while it runs, polled for the progress bar.
  UpdateProgress? updateProgress;
  List<SessionNode> tree = const [];

  /// Sessions whose turn runs on the server, and those waiting on an answer. The open
  /// session's entry follows its event stream; the others come from the list.
  Set<String> running = {};
  Set<String> asking = {};
  List<SessionItem> archivedSessions = const [];
  List<({String name, String path})> archivedFolders = const [];

  /// Sessions whose AI title is being generated.
  final Set<String> naming = {};
  List<Block> blocks = [];
  String shownId = '';
  String initialCwd = '';
  String? turnId;
  String? _textTurnId;
  ConfirmRequest? confirm;
  QuestionRequest? question;
  bool connected = false;
  /// Opening a session: the chat stays blank until its history arrives, like the web.
  bool loading = true;
  bool sending = false;
  String error = '';
  bool unauthorized = false;

  int _view = 0;
  int _revision = 0;
  StreamSubscription<AgentEvent>? _events;
  Timer? _reconnect;
  Timer? _notify;
  Timer? _listTimer;
  bool _disposed = false;

  bool get busy => turnId != null;
  bool get hasReplyText => turnId != null && _textTurnId == turnId;
  String get cwd => status?.cwd.isNotEmpty == true ? status!.cwd : initialCwd;
  String get modelTitle => _titled(status?.model ?? '', status?.modelName ?? '');

  String get sessionName {
    if (shownId.isEmpty) return '新会话';
    for (final root in tree) {
      for (final item in root.allSessions) {
        if (item.id == shownId) return item.prompt.isEmpty ? '空会话' : item.prompt;
      }
    }
    return '空会话';
  }

  String get activity => busy ? (hasReplyText ? '' : '正在回复…') : sending ? '正在发送…' : '';

  Future<void> start() async {
    final mine = _view;
    try {
      final results = await Future.wait([client.status(), client.sessions()]);
      final service = results[0] as Status;
      _list(results[1] as SessionListing);
      initialCwd = service.cwd;
      _server(service);
      if (mine != _view) return;
      final ids = [for (final root in tree) ...root.allSessions.map((item) => item.id)];
      final previous = await ConnectionSettings.lastSession();
      final selected = ids.contains(previous) ? previous : (ids.isEmpty ? null : ids.first);
      if (selected != null) {
        await openSession(id: selected);
      } else {
        connected = true;
        loading = false;
        _changed();
      }
    } catch (reason) {
      if (mine == _view) {
        loading = false;
        _fail(reason, '无法连接 Ashira');
      }
    }
  }

  void _server(Status service) {
    serverVersion = service.version;
    serverUpdate = service.update;
    restartPending = service.restartPending;
    serverInContainer = service.container;
  }

  /// Re-reads the server-wide facts, so an `ashira update` on the server shows its restart
  /// button on an App that is already open.
  Future<void> refreshServer() async {
    _server(await client.status());
    _changed();
  }

  /// Has the server replace its program with the latest release; its restart button follows.
  Future<void> updateServer() async {
    updating = true;
    _changed();
    // Between files (checksum, unpack, replace) nothing downloads: keep the last one shown.
    final poll = Timer.periodic(const Duration(milliseconds: 500), (_) async {
      try {
        final progress = await client.updateProgress();
        if (updating && progress != null) {
          updateProgress = progress;
          _changed();
        }
      } catch (_) {}
    });
    try {
      await client.updateServer();
      _server(await client.status());
    } catch (reason) {
      _fail(reason, '更新服务端失败');
    } finally {
      poll.cancel();
      updating = false;
      updateProgress = null;
      _changed();
    }
  }

  /// Asks the server to re-launch its updated program and waits until it answers again.
  Future<void> restartServer() async {
    restarting = true;
    _changed();
    try {
      await client.restart();
      final before = serverVersion;
      for (var tries = 0; tries < 60; tries++) {
        await Future<void>.delayed(const Duration(seconds: 1));
        final Status service;
        try {
          service = await client.status();
        } catch (_) {
          continue;
        }
        if (service.version != before || !service.restartPending) {
          _server(service);
          return;
        }
      }
      throw Exception('服务端没有在 60 秒内恢复，请检查服务器');
    } catch (reason) {
      _fail(reason, '重启失败');
    } finally {
      restarting = false;
      _changed();
    }
  }

  void _list(SessionListing listing) {
    tree = listing.tree;
    // The open session's own state is fresher from its events than from the list.
    running = {...listing.running.where((id) => id != shownId), if (turnId != null && shownId.isNotEmpty) shownId};
    asking = {...listing.asking.where((id) => id != shownId), if (question != null && shownId.isNotEmpty) shownId};
    archivedSessions = listing.archivedSessions;
    archivedFolders = listing.archivedFolders;
  }

  /// The server announces every list change (`sessions_changed`), including other
  /// devices' edits; a burst of them refreshes once.
  void _listChanged() {
    _listTimer?.cancel();
    _listTimer = Timer(const Duration(milliseconds: 150), () => unawaited(refresh()));
  }

  Future<void> refresh() async {
    try {
      _list(await client.sessions());
      _changed();
    } catch (_) {}
  }

  /// An empty title restores the AI or first-message title.
  Future<void> rename(String id, String title) => _run(() async {
        await client.title(id, title.trim());
        await refresh();
      });

  /// Folds a folder at once here; the server keeps it and tells every other page and App.
  Future<void> setFolded(String path, bool folded) async {
    tree = [for (final root in tree) root.refolded(path, folded)];
    _changed();
    try {
      await client.fold(path, folded: folded);
    } catch (reason) {
      _fail(reason, '折叠状态保存失败');
    }
    await refresh().catchError((Object _) {});
  }

  /// Archiving the shown session moves to the next one left in the list.
  Future<void> setArchived(String kind, String key, {required bool archived}) => _run(() async {
        await client.archive(kind, key, archived: archived);
        await refresh();
        final hidden = archived && (kind == 'session' ? key == shownId : cwd == key);
        if (!hidden) return;
        final next = [for (final root in tree) ...root.allSessions].firstOrNull;
        if (next != null) {
          await openSession(id: next.id);
        } else {
          _view++;
          _closeEvents();
          shownId = '';
          status = null;
          blocks = [];
          turnId = null;
          _changed();
        }
      });

  Future<void> openSession({String? id, bool create = false, String? cwd}) async {
    final mine = ++_view;
    _closeEvents();
    connected = false;
    loading = true;
    turnId = null;
    confirm = null;
    question = null;
    status = null;
    blocks = [];
    shownId = '';
    error = '';
    _changed();
    try {
      final selected = create ? await client.newSession(cwd) : id;
      if (mine != _view || selected == null || selected.isEmpty) return;
      final view = await client.session(selected);
      if (mine != _view) return;
      shownId = selected;
      _apply(view);
      loading = false;
      unawaited(ConnectionSettings.rememberSession(selected));
      _connect(selected, mine, fresh: true);
      if (create) await refresh();
      _changed();
    } catch (reason) {
      if (mine == _view) _fail(reason, '会话操作失败');
    } finally {
      if (mine == _view && loading) {
        loading = false;
        _changed();
      }
    }
  }

  void _apply(SessionView view) {
    status = view.status;
    blocks = view.blocks;
    turnId = view.turnId;
    confirm = view.confirm;
    question = view.question;
  }

  Future<void> _syncHistory() async {
    final mine = _view;
    final snapshot = _revision;
    final id = shownId;
    if (id.isEmpty) return;
    final view = await client.session(id);
    if (mine != _view || snapshot != _revision) return;
    _apply(view);
    _changed();
  }

  /// `fresh`: the history was just loaded; the stream's first `ready` need not fetch it again.
  void _connect(String id, int mine, {bool fresh = false}) {
    _closeEvents();
    // History read before the stream was open can miss events; any one since then means re-read.
    var skipReady = fresh;
    final loadedAt = _revision;
    _events = client.events(id).listen(
      (event) {
        if (mine != _view) return;
        if (event.type == 'ready' || event.type == 'lag') {
          // A (re)connect can follow a server restart or `ashira update`: re-read its state too.
          if (event.type == 'ready') unawaited(refreshServer().catchError((Object _) {}));
          connected = true;
          _changed();
          final first = skipReady;
          skipReady = false;
          if (first && event.type == 'ready' && _revision == loadedAt && !busy) return;
          _syncHistory().catchError((Object reason) {
            if (mine == _view) _fail(reason, '无法加载会话');
          });
        } else if (event.type == 'titling' && event.sessionId != null) {
          // Sent to every page; the new title lands in the same rebuild that drops the spinner.
          final id = event.sessionId!;
          if (event.raw['done'] != true) {
            naming.add(id);
          } else {
            final title = event.field('title');
            if (title != null) tree = [for (final root in tree) root.retitled(id, title)];
            naming.remove(id);
          }
          _changed();
        } else if (event.type == 'sessions_changed') {
          _listChanged();
        } else {
          _onEvent(event);
        }
      },
      onError: (Object reason) {
        if (mine != _view) return;
        if (reason is ApiException && reason.unauthorized) {
          _fail(reason, '需要 API Token');
          return;
        }
        _lost(id, mine);
      },
      onDone: () {
        if (mine == _view) _lost(id, mine);
      },
      cancelOnError: true,
    );
  }

  void _lost(String id, int mine) {
    connected = false;
    _changed();
    _reconnect?.cancel();
    _reconnect = Timer(const Duration(seconds: 2), () {
      if (mine == _view && !_disposed) _connect(id, mine);
    });
  }

  void _closeEvents() {
    _reconnect?.cancel();
    _events?.cancel();
    _events = null;
  }

  AssistantBlock _assistant() {
    final last = blocks.isEmpty ? null : blocks.last;
    if (last is AssistantBlock && last.live) return last;
    final created = AssistantBlock(live: true);
    blocks.add(created);
    return created;
  }

  void _appendText(String text, {required bool reasoning}) {
    if (text.isEmpty) return;
    final parts = _assistant().parts;
    final last = parts.isEmpty ? null : parts.last;
    if (last is TextPart && last.reasoning == reasoning) {
      last.text += text;
    } else {
      parts.add(TextPart(text, reasoning: reasoning));
    }
  }

  void _onEvent(AgentEvent event) {
    if (event.sessionId != shownId) return;
    if (event.type == 'turn_start' && event.turnId != null) {
      _revision++;
      turnId = event.turnId;
      confirm = null;
      question = null;
      error = '';
      running = {...running, shownId};
      _changed();
      return;
    }
    if (event.turnId != null && turnId != null && event.turnId != turnId) return;
    if (event.turnId != null) {
      _revision++;
      turnId = event.turnId;
    }
    switch (event.type) {
      case 'user_message':
        blocks.addAll(historyBlocks([event.raw['message']]));
      case 'text':
        _appendText(event.text, reasoning: false);
        if (event.text.trim().isNotEmpty && event.turnId != null) _textTurnId = event.turnId;
      case 'reasoning':
        _appendText(event.text, reasoning: true);
      case 'tool_start':
        if (!_coordination.contains(event.name)) {
          _assistant().parts.add(ToolPart(name: event.name ?? 'tool', summary: event.field('summary') ?? event.field('intent') ?? ''));
        }
      case 'tool_end':
        if (_coordination.contains(event.name)) {
          if (event.isError) blocks.add(NoteBlock(event.field('output') ?? '子任务失败', error: true));
        } else {
          final tools = _assistant().parts.whereType<ToolPart>().toList();
          final tool = tools.reversed.where((item) => item.name == (event.name ?? item.name) && !item.done).firstOrNull ?? tools.lastOrNull;
          if (tool != null) {
            tool.output = event.field('output') ?? '';
            tool.error = event.isError;
            tool.image = imageUrl(event.raw['image']) ?? tool.image;
            tool.done = true;
          }
        }
      case 'notice':
        blocks.add(NoteBlock(event.text));
      case 'confirm':
        confirm = ConfirmRequest.fromJson(event.raw);
      case 'confirm_cancelled':
        if (confirm?.id == (event.raw['id'] as num?)?.toInt()) confirm = null;
      case 'question':
        question = QuestionRequest.fromJson(event.raw);
        asking = {...asking, shownId};
      case 'question_cancelled':
        if (question?.id == (event.raw['id'] as num?)?.toInt()) {
          question = null;
          asking = {...asking}..remove(shownId);
        }
      case 'subagent':
        final failure = event.field('error');
        final status = switch (event.field('status')) { 'completed' => '已完成', 'failed' => '失败', 'cancelled' => '已取消', final other => other ?? '已结束' };
        blocks.add(NoteBlock('${event.name ?? '未命名'}：$status${failure == null ? '' : '\n$failure'}',
            error: failure != null, agent: event.field('id'), role: event.field('role'), output: event.field('output')));
      case 'peer_message':
        blocks.add(NoteBlock('${event.field('from') ?? '代理'}：${event.field('body') ?? ''}'));
      case 'done':
        turnId = null;
        confirm = null;
        question = null;
        running = {...running}..remove(shownId);
        asking = {...asking}..remove(shownId);
        final current = blocks.isEmpty ? null : blocks.last;
        if (current is AssistantBlock) current.live = false;
        // A stop the user asked for is not a failure worth a red line.
        final failure = event.field('error');
        if (event.raw['ok'] == false && failure != 'cancelled') error = failure ?? '回合失败';
        final mine = _view;
        _syncHistory().catchError((Object reason) {
          if (mine == _view && error.isEmpty) _fail(reason, '无法加载会话');
        });
        unawaited(refresh());
      case 'statistics':
        if (status != null) status = status!.copyWith(statistics: Statistics.fromJson(event.raw));
      case 'context':
        if (status != null && event.raw['tokens'] is num) status = status!.copyWith(context: (event.raw['tokens'] as num).toInt());
      case 'model':
        if (status != null && event.name != null) status = status!.copyWith(model: event.name, modelName: event.field('display') ?? event.name);
    }
    _changed(coalesce: event.type == 'text' || event.type == 'reasoning');
  }

  Future<bool> send(String text, List<({String mediaType, String data})> images, [List<TextFile> files = const []]) async {
    final id = shownId;
    final mine = _view;
    final atSend = _revision;
    if (id.isEmpty || !connected || busy || sending || (text.trim().isEmpty && images.isEmpty && files.isEmpty)) return false;
    error = '';
    sending = true;
    _changed();
    try {
      final accepted = await client.message(id, text.trim(), images, files);
      // A stream event, including done, can arrive before the POST response.
      if (mine == _view && accepted != null && _revision == atSend) {
        _revision++;
        turnId = accepted;
      }
      return true;
    } catch (reason) {
      if (mine == _view) _fail(reason, '发送失败');
      return false;
    } finally {
      sending = false;
      _changed();
    }
  }

  Future<void> decide(String decision) async {
    final pending = confirm;
    final turn = turnId;
    if (pending == null || turn == null || shownId.isEmpty) return;
    confirm = null;
    _changed();
    await _run(() => client.confirm(shownId, turn, pending.id, decision));
  }

  /// `answers == null` skips the group; the model then decides on its own.
  Future<void> answer(Map<String, Object?>? answers) async {
    final pending = question;
    final turn = turnId;
    if (pending == null || turn == null || shownId.isEmpty) return;
    question = null;
    asking = {...asking}..remove(shownId);
    _changed();
    await _run(() => client.answer(shownId, turn, pending.id, answers));
  }

  Future<void> interrupt() async {
    final turn = turnId;
    if (turn != null && shownId.isNotEmpty) await _run(() => client.interrupt(shownId, turn));
  }

  Future<void> setModel(String selector) => _updateSettings((id) => client.setModel(id, selector));

  Future<void> setFast(bool enabled) => _updateSettings((id) => client.setFast(id, enabled));

  Future<void> _updateSettings(Future<void> Function(String id) action) async {
    final id = shownId;
    final mine = _view;
    if (id.isEmpty) return;
    await _run(() async {
      await action(id);
      final view = await client.session(id);
      if (mine == _view) {
        status = view.status;
        _changed();
      }
    });
  }

  Future<void> _run(Future<void> Function() action) async {
    final mine = _view;
    error = '';
    try {
      await action();
    } catch (reason) {
      if (mine == _view) _fail(reason, '操作失败');
    }
  }

  void _fail(Object reason, String fallback) {
    if (reason is ApiException && reason.unauthorized) unauthorized = true;
    error = reason is ApiException ? reason.message : (reason.toString().isEmpty ? fallback : '$fallback：$reason');
    _changed();
  }

  /// Streaming deltas arrive many times per frame; coalesce their rebuilds.
  void _changed({bool coalesce = false}) {
    if (_disposed) return;
    if (!coalesce) {
      _notify?.cancel();
      _notify = null;
      notifyListeners();
      return;
    }
    _notify ??= Timer(const Duration(milliseconds: 33), () {
      _notify = null;
      if (!_disposed) notifyListeners();
    });
  }

  @override
  void dispose() {
    _disposed = true;
    _notify?.cancel();
    _listTimer?.cancel();
    _closeEvents();
    client.close();
    super.dispose();
  }
}

const _levels = ['off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'];

String _titled(String selector, String name) {
  final base = name.isNotEmpty ? name : selector;
  final split = selector.lastIndexOf(':');
  if (base.isEmpty || split <= 0) return base;
  final level = selector.substring(split + 1);
  if (!_levels.contains(level)) return base;
  final label = level[0].toUpperCase() + level.substring(1);
  return base.endsWith(' $label') ? base : '$base $label';
}
