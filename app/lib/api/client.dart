import 'dart:async';
import 'dart:convert';

import 'package:http/http.dart' as http;

import 'models.dart';

class ApiException implements Exception {
  ApiException(this.message, [this.status]);
  final String message;
  final int? status;
  bool get unauthorized => status == 401;
  @override
  String toString() => message;
}

/// HTTP client for one Ashira server. `baseUrl` is e.g. `http://192.168.1.5:8787`.
class AshiraClient {
  AshiraClient({required String baseUrl, String token = ''})
      : baseUrl = baseUrl.replaceAll(RegExp(r'/+$'), ''),
        token = token.trim();

  final String baseUrl;
  final String token;
  final http.Client _http = http.Client();

  void close() => _http.close();

  Map<String, String> get _headers => {
        'content-type': 'application/json',
        if (token.isNotEmpty) 'authorization': 'Bearer $token',
      };

  Uri _uri(String path, [Map<String, String>? query]) => Uri.parse('$baseUrl$path').replace(queryParameters: query);

  static String _session(String id) {
    if (id.isEmpty) throw ApiException('请先选择会话');
    return '/v1/sessions/${Uri.encodeComponent(id)}';
  }

  /// A workspace image of the session for `Image.network`; the token rides in the query.
  Uri rawUri(String sessionId, String path) =>
      _uri('${_session(sessionId)}/raw', {'path': path, if (token.isNotEmpty) 'token': token});

  Future<Object?> _send(String method, String path, {Map<String, String>? query, Object? body}) async {
    final http.Response response;
    try {
      final request = http.Request(method, _uri(path, query))..headers.addAll(_headers);
      if (body != null) request.body = jsonEncode(body);
      response = await http.Response.fromStream(await _http.send(request).timeout(const Duration(seconds: 30)));
    } on TimeoutException {
      throw ApiException('连接 $baseUrl 超时');
    } on Exception catch (error) {
      throw ApiException('无法连接 $baseUrl：$error');
    }
    final text = utf8.decode(response.bodyBytes);
    Object? decoded;
    try {
      decoded = text.isEmpty ? const <String, dynamic>{} : jsonDecode(text);
    } on FormatException {
      if (response.statusCode < 400) throw ApiException('服务返回了无法解析的内容');
    }
    if (response.statusCode >= 400) {
      final message = str(asMap(decoded)['error'], '请求失败（${response.statusCode} ${response.reasonPhrase ?? ''}）'.trim());
      throw ApiException(message, response.statusCode);
    }
    return decoded;
  }

  Future<Object?> _get(String path, [Map<String, String>? query]) => _send('GET', path, query: query);
  Future<Object?> _post(String path, [Object? body]) => _send('POST', path, body: body ?? const <String, dynamic>{});

  Future<Status> status() async => Status.fromJson(await _get('/v1'));

  /// The server re-launches its program (the build `ashira update` installed).
  Future<void> restart() => _post('/v1/restart');

  Future<SessionListing> sessions() async => SessionListing.fromJson(await _get('/v1/sessions'));

  Future<void> alias(String path, String alias) => _post('/v1/alias', {'path': path, 'alias': alias});

  /// An empty title restores the AI or first-message title.
  Future<void> title(String id, String title) => _post('/v1/title', {'id': id, 'title': title});

  /// `kind` is `session` (key = id) or `folder` (key = cwd).
  Future<void> archive(String kind, String key, {required bool archived}) => _post('/v1/archive', {'kind': kind, 'key': key, 'archived': archived});

  /// Folds a folder in the session list for every page and App on this server.
  Future<void> fold(String path, {required bool folded}) => _post('/v1/fold', {'path': path, 'folded': folded});

  Future<String> newSession(String? cwd) async =>
      str(asMap(await _post('/v1/sessions', cwd == null || cwd.isEmpty ? const <String, dynamic>{} : {'cwd': cwd}))['sessionId']);

  Future<SessionView> session(String id) async => SessionView.fromJson(await _get(_session(id)));

  /// Returns the accepted turn id.
  Future<String?> message(String id, String text, List<({String mediaType, String data})> images, [List<TextFile> files = const []]) async {
    final body = await _post('${_session(id)}/messages', {
      'text': text,
      'images': [for (final image in images) {'mediaType': image.mediaType, 'data': image.data}],
      'files': [for (final file in files) {'name': file.name, 'text': file.text}],
    });
    return optStr(asMap(body)['turnId']);
  }

  String _turn(String id, String turn) => '${_session(id)}/turns/${Uri.encodeComponent(turn)}';

  Future<void> interrupt(String id, String turn) => _post('${_turn(id, turn)}/interrupt');

  Future<void> confirm(String id, String turn, int confirmId, String decision) =>
      _post('${_turn(id, turn)}/confirm', {'id': confirmId, 'decision': decision});

  /// `answers == null` skips the group; values are a string, a list of options, or null.
  Future<void> answer(String id, String turn, int questionId, Map<String, Object?>? answers) =>
      _post('${_turn(id, turn)}/answer', {'id': questionId, 'answers': answers});

  Future<void> setModel(String id, String selector) => _post('${_session(id)}/model', {'selector': selector});

  Future<void> setFast(String id, bool enabled) => _post('${_session(id)}/fast', {'enabled': enabled});

  Future<ModelCatalog> models() async => ModelCatalog.fromJson(await _get('/v1/models'));

  Future<FileListing> files(String id, String path) async => FileListing.fromJson(await _get('${_session(id)}/files', {'path': path}));

  Future<void> createEntry(String id, String path, {required bool directory}) =>
      _post('${_session(id)}/files', {'path': path, 'kind': directory ? 'directory' : 'file'});

  Future<String> readFile(String id, String path) async => str(asMap(await _get('${_session(id)}/file', {'path': path}))['text']);

  Future<void> writeFile(String id, String path, String text) => _post('${_session(id)}/file', {'path': path, 'text': text});

  Future<BrowseListing> browse(String path) async => BrowseListing.fromJson(await _get('/v1/browse', {'path': path}));

  Future<SubagentView> subagent(String id) async {
    final body = asMap(await _get('/v1/subagents', {'id': id}));
    return SubagentView(blocks: historyBlocks(body['messages']), report: str(body['report']));
  }

  Future<String> plugins() async => str(asMap(await _get('/v1/plugins'))['text']);

  Future<({List<McpServer> servers, List<String> warnings})> mcp() async {
    final body = asMap(await _get('/v1/mcp'));
    return (
      servers: [
        for (final raw in asList(body['servers'])) McpServer.fromJson(raw),
      ],
      warnings: [for (final warning in asList(body['warnings'])) if (warning is String) warning],
    );
  }

  Future<void> mcpAdd(McpForm form) => _post('/v1/mcp', {'action': 'add', 'scope': 'user', ...form.toJson()});

  Future<void> mcpUpdate(String previous, McpForm form) => _post('/v1/mcp', {'action': 'update', 'previous': previous, ...form.toJson()});

  Future<void> mcpRemove(String name) => _post('/v1/mcp', {'action': 'remove', 'name': name});

  Future<McpForm> mcpConfig(String name) async => McpForm.fromJson(await _get('/v1/mcp/config', {'name': name}));

  Future<({String path, List<MemoryEntry> entries})> memory() async {
    final body = asMap(await _get('/v1/memory'));
    return (
      path: str(body['path']),
      entries: [for (final raw in asList(body['entries'])) MemoryEntry(key: str(asMap(raw)['key']), value: str(asMap(raw)['value']))],
    );
  }

  Future<ModelConfig> modelConfig() async => ModelConfig.fromJson(await _get('/v1/model-config'));

  /// Create (`previous` empty) or replace a provider.
  Future<void> saveProvider(String previous, ProviderForm form) => _post('/v1/model-config', {'action': 'save', 'previous': previous, 'provider': form.toJson()});

  Future<void> removeProvider(String id) => _post('/v1/model-config', {'action': 'remove', 'id': id});

  /// Model ids the form's connection offers, without saving; `previous` lets an empty key use the stored one.
  Future<List<String>> probeModels(String previous, ProviderForm form) async =>
      [for (final model in asList(asMap(await _post('/v1/model-config', {'action': 'probe', 'previous': previous, 'provider': form.toJson()}))['models'])) if (model is String) model];

  /// Saves every role given; an empty selector clears it.
  Future<void> saveRoles(Map<String, String> roles) => _post('/v1/model-config', {'action': 'roles', 'roles': roles});

  Future<UsageReport> usage() async => UsageReport.fromJson(await _get('/v1/usage'));

  /// Server-sent events for one session. The stream ends when the connection
  /// drops; callers reconnect. A `ready` frame arrives first.
  Stream<AgentEvent> events(String id) async* {
    final request = http.Request('GET', _uri('${_session(id)}/events'))
      ..headers.addAll({'accept': 'text/event-stream', if (token.isNotEmpty) 'authorization': 'Bearer $token'});
    final response = await _http.send(request);
    if (response.statusCode != 200) {
      final body = await response.stream.bytesToString();
      throw ApiException(str(asMap(_tryDecode(body))['error'], '事件流连接失败（${response.statusCode}）'), response.statusCode);
    }
    final data = StringBuffer();
    // A silent socket (the server pings every 15 s) means the link is dead.
    final lines = response.stream.timeout(const Duration(seconds: 45)).transform(utf8.decoder).transform(const LineSplitter());
    await for (final line in lines) {
      if (line.isEmpty) {
        if (data.isNotEmpty) {
          final decoded = _tryDecode(data.toString());
          data.clear();
          if (decoded is Map && decoded['type'] is String) yield AgentEvent(decoded.cast<String, dynamic>());
        }
      } else if (line.startsWith('data:')) {
        if (data.isNotEmpty) data.write('\n');
        data.write(line.substring(5).trimLeft());
      }
    }
  }

  static Object? _tryDecode(String text) {
    try {
      return jsonDecode(text);
    } on FormatException {
      return null;
    }
  }
}
