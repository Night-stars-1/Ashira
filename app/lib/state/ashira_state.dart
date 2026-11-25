import 'dart:async';

import 'package:flutter/foundation.dart';

import '../api/client.dart';
import '../api/models.dart';
import 'app_update.dart';
import 'reply_alerts.dart';
import 'settings.dart';

const _coordination = {'task', 'wait', 'task_status', 'task_cancel'};

/// Session state for one server, ported from `web/src/lib/session.ts`.
///
/// `_view` invalidates async work when the shown session changes; `_revision`
/// detects SSE frames that arrived while a history fetch was in flight.
class AshiraState extends ChangeNotifier {
  AshiraState(this.settings) : client = AshiraClient(baseUrl: settings.baseUrl, token: settings.token) {
    _alerts = ReplyAlerts(onOpen: (id) {
      if (id != shownId) unawaited(openSession(id: id));
    });
  }

  late final ReplyAlerts _alerts;

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
  /// 设置 → 通用 hid the chat's approval switch: sessions run in the default mode.
  bool approvalHidden = false;
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
  final Set<String> _approvalRequests = {};
  String error = '';
  /// [error] is a failed turn, so 重试 can ask the model again.
  bool errorRetry = false;
  /// Messages sent while the reply ran, until the model reads them or hands them back.
  List<String> pendingSteer = [];
  /// The text of steered messages the turn ended without reading; the composer takes it back.
  String? returnedDraft;
  /// History paging: the index of the first block shown, and how many the session has.
  int historyStart = 0;
  int historyTotal = 0;
  bool loadingEarlier = false;
  bool unauthorized = false;

  int _view = 0;
  int _revision = 0;
  // Approval changes run outside turns, so they must not invalidate an in-flight send.
  int _approvalRevision = 0;
  StreamSubscription<AgentEvent>? _events;
  Timer? _reconnect;
  Timer? _notify;
  Timer? _listTimer;
  bool _disposed = false;

  bool get busy => turnId != null;
  bool get hasReplyText => turnId != null && _textTurnId == turnId;
  String get cwd => status?.cwd.isNotEmpty == true ? status!.cwd : initialCwd;
  String get modelTitle => _titled(status?.model ?? '', status?.modelName ?? '');
  bool get approvalSubmitting => _approvalRequests.contains(shownId);
  bool get canSetApprovalMode => !(_disposed || unauthorized || loading || busy || sending || approvalSubmitting) &&
      connected && shownId.isNotEmpty && status != null;

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
      // A tapped notification opened the App: its session, else the last one shown.
      final tapped = await ReplyAlerts.launchSession();
      final previous = ids.contains(tapped) ? tapped : await ConnectionSettings.lastSession();
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
    approvalHidden = service.approvalHidden;
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
    _syncAlerts();
  }

  void _syncAlerts() => _alerts.sync(running: running, asking: asking, shown: shownId, title: _title);

  String _title(String id) {
    for (final root in tree) {
      for (final item in root.allSessions) {
        if (item.id == id) return item.prompt.isEmpty ? '空会话' : item.prompt;
      }
    }
    return 'Ashira';
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
    historyStart = 0;
    historyTotal = 0;
    pendingSteer = [];
    shownId = '';
    _clearError();
    _changed();
    try {
      final selected = create ? await client.newSession(cwd) : id;
      if (mine != _view || selected == null || selected.isEmpty) return;
      final view = await client.session(selected, limit: _historyPage);
      if (mine != _view) return;
      shownId = selected;
      _apply(view);
      loading = false;
      unawaited(ConnectionSettings.rememberSession(selected));
      _alerts.seen(selected);
      _syncAlerts();
      _connect(selected, mine);
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

  void _setStatus(Status value, {int? approvalSnapshot}) {
    final currentMode = status?.approvalMode;
    status = currentMode != null && approvalSnapshot != null && approvalSnapshot != _approvalRevision
        ? value.copyWith(approvalMode: currentMode)
        : value;
  }

  void _apply(SessionView view, {int? approvalSnapshot}) {
    _setStatus(view.status, approvalSnapshot: approvalSnapshot);
    blocks = view.blocks;
    historyStart = view.start;
    historyTotal = view.total;
    turnId = view.turnId;
    confirm = view.confirm;
    question = view.question;
    // A question or an end missed while the event stream reconnected shows here first.
    if (shownId.isNotEmpty) {
      running = {...running.where((id) => id != shownId), if (turnId != null) shownId};
      asking = {...asking.where((id) => id != shownId), if (turnId != null && question != null) shownId};
      _syncAlerts();
    }
  }

  Future<void> _syncHistory() async {
    final mine = _view;
    final snapshot = _revision;
    final approvalSnapshot = _approvalRevision;
    final id = shownId;
    if (id.isEmpty) return;
    // Keep the pages already scrolled back to: re-read from the same first block.
    final limit = historyTotal - historyStart + 50 > _historyPage ? historyTotal - historyStart + 50 : _historyPage;
    final view = await client.session(id, limit: limit);
    if (mine != _view || snapshot != _revision) return;
    _apply(view, approvalSnapshot: approvalSnapshot);
    _changed();
  }

  /// The page before the first block shown, prepended; returns whether it added any.
  Future<bool> loadEarlier() async {
    final id = shownId;
    final mine = _view;
    if (id.isEmpty || loadingEarlier || historyStart == 0) return false;
    loadingEarlier = true;
    _changed();
    try {
      final view = await client.session(id, before: historyStart, limit: _historyPage);
      if (mine != _view) return false;
      blocks = [...view.blocks, ...blocks];
      historyStart = view.start;
      return view.blocks.isNotEmpty;
    } catch (reason) {
      if (mine == _view) _fail(reason, '无法加载更早的消息');
      return false;
    } finally {
      if (mine == _view) {
        loadingEarlier = false;
        _changed();
      }
    }
  }

  void _connect(String id, int mine) {
    _closeEvents();
    _events = client.events(id).listen(
      (event) {
        if (mine != _view) return;
        if (event.type == 'ready' || event.type == 'lag') {
          // A (re)connect can follow a server restart or `ashira update`: re-read its state too.
          if (event.type == 'ready') unawaited(refreshServer().catchError((Object _) {}));
          connected = true;
          _changed();
          // Re-read on the first ready too: settings may have changed before the stream subscribed.
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
    if (event.type == 'approval_mode') {
      final mode = event.field('approvalMode');
      if (mode != null && status != null && (mode == 'manual' || mode == 'auto' || mode == 'full')) {
        _approvalRevision++;
        status = status!.copyWith(approvalMode: mode);
        _changed();
      }
      return;
    }
    if (event.type == 'turn_start' && event.turnId != null) {
      _revision++;
      turnId = event.turnId;
      confirm = null;
      question = null;
      _clearError();
      running = {...running, shownId};
      _syncAlerts();
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
      case 'steered':
        // Shown where the model read them; the pending copies go as they arrive.
        final read = historyBlocks(event.raw['messages']);
        pendingSteer = pendingSteer.skip(read.length).toList();
        final current = blocks.isEmpty ? null : blocks.last;
        if (current is AssistantBlock) current.live = false;
        blocks.addAll(read);
      case 'steer_returned':
        // The turn ended before reading them: back into the composer to send next.
        final texts = [for (final block in historyBlocks(event.raw['messages'])) if (block is UserBlock && block.text.isNotEmpty) block.text];
        pendingSteer = [];
        if (texts.isNotEmpty) returnedDraft = texts.join('\n\n');
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
          // A stop the user asked for interrupts `wait`; that is not a failure.
          if (event.isError && event.field('output') != 'cancelled') blocks.add(NoteBlock(event.field('output') ?? '子任务失败', error: true));
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
        final asked = question?.questions.map((item) => item.question).join('\n') ?? '';
        _alerts.asked(shownId, sessionName, asked);
        _syncAlerts();
      case 'question_cancelled':
        if (question?.id == (event.raw['id'] as num?)?.toInt()) {
          question = null;
          asking = {...asking}..remove(shownId);
          _syncAlerts();
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
        // Unread steers came back in `steer_returned` just before; any copy left is stale
        // (its POST answered after the model had already read it).
        pendingSteer = [];
        confirm = null;
        question = null;
        running = {...running}..remove(shownId);
        asking = {...asking}..remove(shownId);
        final current = blocks.isEmpty ? null : blocks.last;
        if (current is AssistantBlock) current.live = false;
        // A stop the user asked for is not a failure worth a red line.
        final failure = event.field('error');
        if (event.raw['ok'] == false && failure != 'cancelled') {
          error = failure ?? '回合失败';
          errorRetry = true;
        }
        // The answer: the text after the last tool, as the transcript shows it.
        final parts = current is AssistantBlock ? current.parts : const <ReplyPart>[];
        final answer = parts.skip(parts.lastIndexWhere((part) => part is ToolPart) + 1).whereType<TextPart>().where((part) => !part.reasoning).map((part) => part.text).join('\n');
        _alerts.finished(shownId, sessionName, error: event.raw['ok'] == false ? failure ?? '回合失败' : null, answer: answer);
        _syncAlerts();
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

  /// While the reply runs the message joins that turn (`steered`) instead of starting one.
  Future<bool> send(String text, List<({String mediaType, String data})> images, [List<TextFile> files = const []]) async {
    final id = shownId;
    final mine = _view;
    final atSend = _revision;
    if (id.isEmpty || !connected || sending || approvalSubmitting || (text.trim().isEmpty && images.isEmpty && files.isEmpty)) return false;
    _clearError();
    sending = true;
    _changed();
    try {
      final accepted = await client.message(id, text.trim(), images, files);
      if (mine == _view) {
        if (accepted.steered) {
          pendingSteer = [...pendingSteer, text.trim().isEmpty ? '（附件）' : text.trim()];
        } else if (accepted.turnId != null && _revision == atSend) {
          // A stream event, including done, can arrive before the POST response.
          _revision++;
          turnId = accepted.turnId;
        }
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

  /// 重试 on a failed turn: the model answers again from the history as it stands.
  Future<void> retry() async {
    final id = shownId;
    final mine = _view;
    final atSend = _revision;
    if (id.isEmpty || busy) return;
    _clearError();
    _changed();
    try {
      final accepted = await client.retry(id);
      if (mine == _view && accepted != null && _revision == atSend) {
        _revision++;
        turnId = accepted;
        _changed();
      }
    } catch (reason) {
      if (mine == _view) {
        _fail(reason, '重试失败');
        errorRetry = true;
      }
    }
  }

  void dismissError() {
    _clearError();
    _changed();
  }

  void _clearError() {
    error = '';
    errorRetry = false;
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
    _syncAlerts();
    _changed();
    await _run(() => client.answer(shownId, turn, pending.id, answers));
  }

  Future<void> interrupt() async {
    final turn = turnId;
    if (turn != null && shownId.isNotEmpty) await _run(() => client.interrupt(shownId, turn));
  }

  Future<void> setModel(String selector) => _updateSettings((id) => client.setModel(id, selector));

  Future<void> setFast(bool enabled) => _updateSettings((id) => client.setFast(id, enabled));

  /// 设置 → 通用 saved `approval:`; hiding moved idle sessions to the default, so re-read this one.
  void approvalSaved(ApprovalSettings value) {
    approvalHidden = value.hidden;
    _changed();
    unawaited(_updateSettings((_) async {}));
  }

  Future<void> setApprovalMode(String mode, {Future<bool> Function()? confirmFullAccess}) async {
    if (!canSetApprovalMode || status?.approvalMode == mode) return;
    final id = shownId;
    final mine = _view;
    if (mode == 'full' && (confirmFullAccess == null || !await confirmFullAccess())) return;
    if (mine != _view || !canSetApprovalMode || status?.approvalMode == mode) return;
    final approvalSnapshot = _approvalRevision;
    _approvalRequests.add(id);
    error = '';
    _changed();
    try {
      final saved = await client.setApprovalMode(id, mode);
      // A live update received during the request may be newer than its response.
      if (!_disposed && mine == _view && _approvalRevision == approvalSnapshot) {
        status = status?.copyWith(approvalMode: saved);
        _approvalRevision++;
      }
    } catch (reason) {
      if (!_disposed && mine == _view) _fail(reason, '切换审批模式失败');
    } finally {
      _approvalRequests.remove(id);
      if (shownId == id) _changed();
    }
  }

  Future<void> _updateSettings(Future<void> Function(String id) action) async {
    final id = shownId;
    final mine = _view;
    if (id.isEmpty) return;
    await _run(() async {
      await action(id);
      final approvalSnapshot = _approvalRevision;
      final view = await client.session(id);
      if (mine == _view) {
        _setStatus(view.status, approvalSnapshot: approvalSnapshot);
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
    errorRetry = false;
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
    _alerts.dispose();
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

/// Blocks a history page holds: a long session opens with its end and pages back.
const _historyPage = 200;
