// Typed views of the Ashira HTTP API (`src/serve.rs`). Parsing is lenient:
// missing or mistyped fields fall back to empty values instead of throwing,
// matching the web client in `web/src/lib/api.ts`.

Map<String, dynamic> asMap(Object? value) => value is Map ? value.cast<String, dynamic>() : const {};
List<dynamic> asList(Object? value) => value is List ? value : const [];
String str(Object? value, [String fallback = '']) => value is String ? value : fallback;
String? optStr(Object? value) => value is String && value.isNotEmpty ? value : null;
double num0(Object? value) => value is num ? value.toDouble() : 0;
int int0(Object? value) => value is num ? value.toInt() : 0;
double? metric(Object? value) => value is num && value.isFinite && value >= 0 ? value.toDouble() : null;

class Usage {
  const Usage({this.input = 0, this.output = 0, this.cacheRead = 0, this.cacheWrite = 0});
  factory Usage.fromJson(Object? json) {
    final m = asMap(json);
    return Usage(input: int0(m['input']), output: int0(m['output']), cacheRead: int0(m['cacheRead']), cacheWrite: int0(m['cacheWrite']));
  }
  final int input, output, cacheRead, cacheWrite;
  int get prompt => input + cacheRead + cacheWrite;
  int get total => prompt + output;
}

class Cost {
  const Cost({this.main = 0, this.child = 0, this.mainUnknown = false, this.childUnknown = false});
  factory Cost.fromJson(Object? json) {
    final m = asMap(json);
    return Cost(main: num0(m['main']), child: num0(m['child']), mainUnknown: m['mainUnknown'] == true, childUnknown: m['childUnknown'] == true);
  }
  final double main, child;
  final bool mainUnknown, childUnknown;
}

class Price {
  const Price({this.input, this.output, this.cacheRead, this.cacheWrite});
  factory Price.fromJson(Object? json) {
    final m = asMap(json);
    return Price(input: metric(m['input']), output: metric(m['output']), cacheRead: metric(m['cacheRead']), cacheWrite: metric(m['cacheWrite']));
  }
  final double? input, output, cacheRead, cacheWrite;
}

class Statistics {
  const Statistics({this.usage = const Usage(), this.cost = const Cost(), this.mainRequests = 0, this.childRequests = 0, this.lastRate});
  factory Statistics.fromJson(Map<String, dynamic> m) {
    final requests = asMap(m['requests']);
    return Statistics(
      usage: Usage.fromJson(m['usage']),
      cost: Cost.fromJson(m['cost']),
      mainRequests: int0(requests['main']),
      childRequests: int0(requests['child']),
      lastRate: metric(m['lastRate']),
    );
  }
  final Usage usage;
  final Cost cost;
  final int mainRequests, childRequests;
  final double? lastRate;
}

class Status {
  const Status({
    required this.cwd,
    required this.sessionId,
    this.model,
    this.modelName,
    this.busy = false,
    this.fast = false,
    this.context = 0,
    this.window,
    this.price,
    this.statistics = const Statistics(),
  });
  factory Status.fromJson(Object? json) {
    final m = asMap(json);
    return Status(
      cwd: str(m['cwd']),
      sessionId: str(m['sessionId']),
      model: optStr(m['model']),
      modelName: optStr(m['modelName']),
      busy: m['busy'] == true,
      fast: m['fast'] == true,
      context: int0(m['context']),
      window: metric(m['window'])?.toInt(),
      price: m['price'] == null ? null : Price.fromJson(m['price']),
      statistics: Statistics.fromJson(m),
    );
  }
  final String cwd, sessionId;
  final String? model, modelName;
  final bool busy, fast;
  final int context;
  final int? window;
  final Price? price;
  final Statistics statistics;

  Status copyWith({String? model, String? modelName, int? context, Statistics? statistics}) => Status(
        cwd: cwd,
        sessionId: sessionId,
        model: model ?? this.model,
        modelName: modelName ?? this.modelName,
        busy: busy,
        fast: fast,
        context: context ?? this.context,
        window: window,
        price: price,
        statistics: statistics ?? this.statistics,
      );
}

class ConfirmRequest {
  const ConfirmRequest({required this.id, required this.name, required this.command, required this.reason, required this.always});
  static ConfirmRequest? fromJson(Object? json) {
    final m = asMap(json);
    final id = m['id'];
    if (id is! num) return null;
    return ConfirmRequest(id: id.toInt(), name: str(m['name'], '命令'), command: str(m['command']), reason: str(m['reason']), always: str(m['always'], '始终允许'));
  }
  final int id;
  final String name, command, reason, always;
}

enum QuestionKind { single, multi, text }

class QuestionOption {
  const QuestionOption(this.label, [this.description = '']);
  static QuestionOption? fromJson(Object? json) {
    if (json is String) return json.isEmpty ? null : QuestionOption(json);
    final m = asMap(json);
    final label = str(m['label']);
    return label.isEmpty ? null : QuestionOption(label, str(m['description']));
  }
  /// Title; answers refer to the option by it.
  final String label;
  /// Optional note shown under the title; empty when absent.
  final String description;
}

class QuestionItem {
  const QuestionItem({required this.id, required this.question, required this.kind, required this.options, required this.defaults});
  static QuestionItem? fromJson(Object? json) {
    final m = asMap(json);
    final id = str(m['id']);
    final question = str(m['question']);
    if (id.isEmpty || question.isEmpty) return null;
    List<String> strings(Object? v) => (v is List ? v : [v]).whereType<String>().where((s) => s.isNotEmpty).toList();
    final kind = switch (m['type']) { 'multi' => QuestionKind.multi, 'text' => QuestionKind.text, _ => QuestionKind.single };
    final options = m['options'] is List ? (m['options'] as List).map(QuestionOption.fromJson).whereType<QuestionOption>().toList() : const <QuestionOption>[];
    return QuestionItem(id: id, question: question, kind: kind, options: options, defaults: strings(m['default']));
  }
  final String id, question;
  final QuestionKind kind;
  final List<QuestionOption> options;
  /// Text: the prefilled answer. Choices: labels of preselected options.
  final List<String> defaults;
}

/// A pending `ask_user` group.
class QuestionRequest {
  const QuestionRequest({required this.id, required this.questions});
  static QuestionRequest? fromJson(Object? json) {
    final m = asMap(json);
    final id = m['id'];
    if (id is! num || m['questions'] is! List) return null;
    final questions = (m['questions'] as List).map(QuestionItem.fromJson).whereType<QuestionItem>().toList();
    return questions.isEmpty ? null : QuestionRequest(id: id.toInt(), questions: questions);
  }
  final int id;
  final List<QuestionItem> questions;
}

class SessionView {
  const SessionView({required this.status, required this.blocks, this.turnId, this.confirm, this.question});
  factory SessionView.fromJson(Object? json) {
    final m = asMap(json);
    return SessionView(
      status: Status.fromJson(m),
      blocks: historyBlocks(m['messages']),
      turnId: optStr(m['turnId']),
      confirm: m['confirm'] == null ? null : ConfirmRequest.fromJson(m['confirm']),
      question: m['question'] == null ? null : QuestionRequest.fromJson(m['question']),
    );
  }
  final Status status;
  final List<Block> blocks;
  final String? turnId;
  final ConfirmRequest? confirm;
  final QuestionRequest? question;
}

class SessionItem {
  const SessionItem({required this.id, required this.prompt, required this.modified, this.renamed = false, this.cwd = ''});
  factory SessionItem.fromJson(Map<String, dynamic> m) =>
      SessionItem(id: str(m['id']), prompt: str(m['prompt']), modified: int0(m['modified']), renamed: m['renamed'] == true, cwd: str(m['cwd']));
  final String id, prompt;

  /// Unix milliseconds; 0 when unknown.
  final int modified;

  /// You named it; an AI title must not overwrite it.
  final bool renamed;

  /// Set on archived rows only.
  final String cwd;

  SessionItem withPrompt(String prompt) => SessionItem(id: id, prompt: prompt, modified: modified, renamed: renamed, cwd: cwd);
}

/// `GET /v1/sessions`: the tree, the sessions with a turn running (and those waiting
/// for an answer), and the archive.
class SessionListing {
  const SessionListing({required this.tree, this.running = const {}, this.asking = const {}, this.archivedSessions = const [], this.archivedFolders = const []});
  factory SessionListing.fromJson(Object? json) {
    final m = asMap(json);
    final archived = asMap(m['archived']);
    Set<String> ids(Object? list) => {for (final id in asList(list)) if (id is String && id.isNotEmpty) id};
    return SessionListing(
      tree: [for (final root in asList(m['roots'])) ?SessionNode.fromJson(root)],
      running: ids(m['running']),
      asking: ids(m['asking']),
      archivedSessions: [for (final raw in asList(archived['sessions'])) if (str(asMap(raw)['id']).isNotEmpty) SessionItem.fromJson(asMap(raw))],
      archivedFolders: [
        for (final raw in asList(archived['folders']))
          if (str(asMap(raw)['path']).isNotEmpty) (name: str(asMap(raw)['name'], str(asMap(raw)['path'])), path: str(asMap(raw)['path'])),
      ],
    );
  }
  final List<SessionNode> tree;
  final Set<String> running, asking;
  final List<SessionItem> archivedSessions;
  final List<({String name, String path})> archivedFolders;
}

class SessionNode {
  const SessionNode({required this.name, required this.path, required this.sessions, required this.children});
  static SessionNode? fromJson(Object? json) {
    final m = asMap(json);
    final path = str(m['path']);
    if (path.isEmpty) return null;
    return SessionNode(
      name: str(m['name'], path),
      path: path,
      sessions: [
        for (final item in asList(m['sessions']))
          if (str(asMap(item)['id']).isNotEmpty) SessionItem.fromJson(asMap(item)),
      ],
      children: [for (final child in asList(m['children'])) ?SessionNode.fromJson(child)],
    );
  }
  final String name, path;
  final List<SessionItem> sessions;

  /// Same tree with one session's title replaced (for a finished AI title).
  SessionNode retitled(String id, String title) => SessionNode(
        name: name,
        path: path,
        sessions: [for (final item in sessions) item.id == id && !item.renamed ? item.withPrompt(title) : item],
        children: [for (final child in children) child.retitled(id, title)],
      );
  final List<SessionNode> children;

  Iterable<SessionItem> get allSessions sync* {
    yield* sessions;
    for (final child in children) {
      yield* child.allSessions;
    }
  }
}

class ReplyMetrics {
  const ReplyMetrics({this.model, this.usage, this.rate, this.cost, this.durationMs});
  static ReplyMetrics? fromJson(Object? json) {
    if (json is! Map) return null;
    final m = asMap(json);
    return ReplyMetrics(
      model: optStr(m['model']),
      usage: m['usage'] == null ? null : Usage.fromJson(m['usage']),
      rate: metric(m['rate']),
      cost: metric(m['cost']),
      durationMs: metric(m['durationMs']),
    );
  }
  final String? model;
  final Usage? usage;
  final double? rate, cost, durationMs;
}

sealed class ReplyPart {}

class TextPart extends ReplyPart {
  TextPart(this.text, {this.reasoning = false});
  String text;
  final bool reasoning;
}

class ToolPart extends ReplyPart {
  ToolPart({required this.name, required this.summary, this.output = '', this.error = false, this.done = false});
  final String name, summary;
  String output;
  bool error, done;
}

sealed class Block {}

/// A text file attached to a message; `lines` counts its lines for the card.
class TextFile {
  const TextFile({required this.name, required this.text, required this.lines});
  factory TextFile.of(String name, String text) => TextFile(name: name, text: text, lines: '\n'.allMatches(text).length + 1);
  final String name, text;
  final int lines;
}

class UserBlock extends Block {
  UserBlock(this.text, [this.images = const [], this.files = const []]);
  final String text;

  /// `data:` URLs.
  final List<String> images;
  final List<TextFile> files;
}

class NoteBlock extends Block {
  NoteBlock(this.text, {this.error = false, this.agent});
  final String text;
  final bool error;
  final String? agent;
}

class AssistantBlock extends Block {
  AssistantBlock({List<ReplyPart>? parts, this.live = false, this.metrics}) : parts = parts ?? [];
  final List<ReplyPart> parts;
  bool live;
  final ReplyMetrics? metrics;

  String get text => parts.whereType<TextPart>().where((part) => !part.reasoning).map((part) => part.text).join('\n');
}

List<Block> historyBlocks(Object? messages) {
  final blocks = <Block>[];
  for (final item in asList(messages)) {
    final row = asMap(item);
    switch (row['kind']) {
      case 'user':
        blocks.add(UserBlock(
          str(row['text']),
          [for (final image in asList(row['images'])) if (image is String) image],
          [
            for (final raw in asList(row['files']))
              if (asMap(raw)['text'] is String)
                TextFile(
                  name: str(asMap(raw)['name'], '附件'),
                  text: str(asMap(raw)['text']),
                  lines: asMap(raw)['lines'] is num ? int0(asMap(raw)['lines']) : '\n'.allMatches(str(asMap(raw)['text'])).length + 1,
                ),
          ],
        ));
      case 'note':
        blocks.add(NoteBlock(str(row['text']), error: row['error'] == true, agent: optStr(row['agent'])));
      case 'assistant':
        final parts = <ReplyPart>[];
        for (final raw in asList(row['parts'])) {
          final part = asMap(raw);
          final type = part['type'];
          if ((type == 'text' || type == 'reasoning') && str(part['text']).isNotEmpty) {
            parts.add(TextPart(str(part['text']), reasoning: type == 'reasoning'));
          } else if (type == 'tool') {
            parts.add(ToolPart(name: str(part['name'], 'tool'), summary: str(part['summary']), output: str(part['output']), error: part['error'] == true, done: part['done'] != false));
          }
        }
        blocks.add(AssistantBlock(parts: parts, metrics: ReplyMetrics.fromJson(row['metrics'])));
    }
  }
  return blocks;
}

class ModelItem {
  const ModelItem({required this.selector, required this.provider, required this.id, this.name, this.context = 0, this.input, this.output, this.fast = false, this.efforts = const []});
  final String selector, provider, id;
  final String? name;
  final int context;
  final double? input, output;
  /// `/fast` applies (GPT on Chat Completions or Responses).
  final bool fast;

  /// Thinking levels the model accepts; empty when unknown (offer all).
  final List<String> efforts;
}

class ModelCatalog {
  const ModelCatalog({required this.current, required this.levels, required this.models});
  factory ModelCatalog.fromJson(Object? json) {
    final m = asMap(json);
    final levels = [for (final level in asList(m['levels'])) if (level is String) level];
    return ModelCatalog(
      current: optStr(m['current']),
      levels: levels.isEmpty ? const ['默认'] : levels,
      models: [
        for (final raw in asList(m['models']))
          ModelItem(
            selector: str(asMap(raw)['selector']),
            provider: str(asMap(raw)['provider']),
            id: str(asMap(raw)['id']),
            name: optStr(asMap(raw)['name']),
            context: int0(asMap(raw)['context']),
            input: asMap(raw)['input'] is num ? (asMap(raw)['input'] as num).toDouble() : null,
            output: asMap(raw)['output'] is num ? (asMap(raw)['output'] as num).toDouble() : null,
            fast: asMap(raw)['fast'] == true,
            efforts: [for (final effort in asList(asMap(raw)['efforts'])) if (effort is String) effort],
          ),
      ],
    );
  }
  final String? current;
  final List<String> levels;
  final List<ModelItem> models;
}

class FileEntry {
  const FileEntry({required this.name, required this.dir});
  final String name;
  final bool dir;
}

class FileListing {
  const FileListing({required this.path, required this.entries});
  factory FileListing.fromJson(Object? json) {
    final m = asMap(json);
    return FileListing(path: str(m['path']), entries: [for (final raw in asList(m['entries'])) FileEntry(name: str(asMap(raw)['name']), dir: asMap(raw)['dir'] == true)]);
  }
  final String path;
  final List<FileEntry> entries;
}

class BrowseEntry {
  const BrowseEntry({required this.name, required this.path});
  final String name, path;
}

class BrowseListing {
  const BrowseListing({required this.path, this.parent, required this.entries});
  factory BrowseListing.fromJson(Object? json) {
    final m = asMap(json);
    return BrowseListing(
      path: str(m['path']),
      parent: optStr(m['parent']),
      entries: [
        for (final raw in asList(m['entries']))
          if (str(asMap(raw)['path']).isNotEmpty) BrowseEntry(name: str(asMap(raw)['name']), path: str(asMap(raw)['path'])),
      ],
    );
  }
  final String path;
  final String? parent;
  final List<BrowseEntry> entries;
}

class McpServer {
  const McpServer({required this.name, required this.status, required this.kind, required this.source, this.target = '', this.tools = const []});
  factory McpServer.fromJson(Object? json) {
    final m = asMap(json);
    return McpServer(
      name: str(m['name']),
      status: str(m['status']),
      kind: str(m['kind']),
      source: str(m['source']),
      target: str(m['target']),
      tools: [for (final raw in asList(m['tools'])) (name: str(asMap(raw)['name']), description: str(asMap(raw)['description']))],
    );
  }
  final String name, status, kind, source, target;
  final List<({String name, String description})> tools;

  /// Error text after `failed: `, empty otherwise.
  String get failure => status.startsWith('failed: ') ? status.substring('failed: '.length) : '';
}

/// The editable fields of one MCP definition, in the add/edit form's shape.
class McpForm {
  const McpForm({this.name = '', this.type = 'stdio', this.command = '', this.args = '', this.url = '', this.headers = ''});
  factory McpForm.fromJson(Object? json) {
    final m = asMap(json);
    final type = str(m['type']);
    return McpForm(
      name: str(m['name']),
      type: type == 'http' || type == 'sse' ? type : 'stdio',
      command: str(m['command']),
      args: str(m['args']),
      url: str(m['url']),
      headers: str(m['headers']),
    );
  }
  final String name, type, command, args, url, headers;
  Map<String, String> toJson() => {'name': name, 'type': type, 'command': command, 'args': args, 'url': url, 'headers': headers};
}

class MemoryEntry {
  const MemoryEntry({required this.key, required this.value});
  final String key, value;
}

class SubagentView {
  const SubagentView({required this.blocks, required this.report});
  final List<Block> blocks;
  final String report;
}

/// One SSE frame. Fields are read lazily from the raw map; see `encode` in `src/serve.rs`.
class AgentEvent {
  AgentEvent(this.raw);
  final Map<String, dynamic> raw;
  String get type => str(raw['type']);
  String? get sessionId => optStr(raw['sessionId']);
  String? get turnId => optStr(raw['turnId']);
  String get text => str(raw['text']);
  String? get name => optStr(raw['name']);
  String? field(String key) => optStr(raw[key]);
  bool get isError => raw['isError'] == true;
}

/// How a provider gets its key; a stored literal key is never sent to the app.
class ProviderKey {
  const ProviderKey(this.kind, [this.value = '', this.unset = false]);

  /// `env`, `command`, `literal`, `none` or `missing`.
  final String kind, value;

  /// An `env` name that is not set on the server.
  final bool unset;
}

class ProviderStatus {
  const ProviderStatus({required this.state, this.detail, this.models = 0});

  /// `ready`, `pending` or `failed`.
  final String state;
  final String? detail;
  final int models;
}

class ProviderConfig {
  const ProviderConfig({required this.id, required this.api, required this.baseUrl, required this.key, required this.discovery, required this.models, this.status});
  final String id, api, baseUrl;
  final ProviderKey key;
  final bool discovery;
  final List<String> models;
  final ProviderStatus? status;
}

/// `GET /v1/model-config`: providers from models.yml plus the default/task roles.
class ModelConfig {
  const ModelConfig({required this.apis, required this.providers, required this.defaultRole, required this.taskRole, this.loadError, this.warnings = const []});
  factory ModelConfig.fromJson(Object? json) {
    final m = asMap(json);
    return ModelConfig(
      apis: [for (final api in asList(m['apis'])) if (api is String) api],
      providers: [
        for (final raw in asList(m['providers']))
          ProviderConfig(
            id: str(asMap(raw)['id']),
            api: str(asMap(raw)['api']),
            baseUrl: str(asMap(raw)['baseUrl']),
            key: ProviderKey(str(asMap(asMap(raw)['key'])['kind'], 'missing'), str(asMap(asMap(raw)['key'])['value']), asMap(asMap(raw)['key'])['unset'] == true),
            discovery: asMap(raw)['discovery'] == true,
            models: [for (final model in asList(asMap(raw)['models'])) if (model is String) model],
            status: asMap(raw)['status'] is Map
                ? ProviderStatus(state: str(asMap(asMap(raw)['status'])['state']), detail: optStr(asMap(asMap(raw)['status'])['detail']), models: int0(asMap(asMap(raw)['status'])['models']))
                : null,
          ),
      ],
      defaultRole: str(m['default']),
      taskRole: str(m['task']),
      loadError: optStr(m['loadError']),
      warnings: [for (final warning in asList(m['warnings'])) if (warning is String) warning],
    );
  }
  final List<String> apis;
  final List<ProviderConfig> providers;
  final String defaultRole, taskRole;
  final String? loadError;
  final List<String> warnings;
}

/// What the provider form sends; an empty `keyValue` keeps the stored key.
class ProviderForm {
  const ProviderForm({required this.id, required this.api, required this.baseUrl, required this.keyKind, required this.keyValue, required this.discovery, required this.models});
  final String id, api, baseUrl, keyKind, keyValue;
  final bool discovery;
  final List<String> models;
  Map<String, Object?> toJson() =>
      {'id': id, 'api': api, 'baseUrl': baseUrl, 'key': {'kind': keyKind, 'value': keyValue}, 'discovery': discovery, 'models': models};
}

/// One UTC hour of one model across all sessions; `hour` is unix seconds / 3600.
class UsageBucket {
  const UsageBucket({required this.hour, required this.model, required this.usage, required this.cost, required this.unknown, required this.requests});
  final int hour;
  final String model;
  final Usage usage;
  final double cost;
  final int unknown, requests;
}

/// `GET /v1/usage`.
class UsageReport {
  const UsageReport({required this.sessions, required this.buckets});
  factory UsageReport.fromJson(Object? json) {
    final m = asMap(json);
    return UsageReport(
      sessions: int0(m['sessions']),
      buckets: [
        for (final raw in asList(m['buckets']))
          UsageBucket(
            hour: int0(asMap(raw)['hour']),
            model: str(asMap(raw)['model'], '未知模型'),
            usage: Usage.fromJson(raw),
            cost: num0(asMap(raw)['cost']),
            unknown: int0(asMap(raw)['unknown']),
            requests: int0(asMap(raw)['requests']),
          ),
      ],
    );
  }
  final int sessions;
  final List<UsageBucket> buckets;
}
