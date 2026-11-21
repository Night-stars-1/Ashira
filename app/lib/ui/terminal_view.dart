import 'dart:async';
import 'dart:convert';
import 'dart:io' show WebSocket;

import 'package:flutter/material.dart';
import 'package:xterm/xterm.dart';

import '../api/client.dart';
import 'theme.dart';

/// The session's terminals: a tab strip over one shell per tab, each its own
/// `/v1/sessions/{id}/terminal` connection (its own PTY on the server), like the web.
class SessionTerminal extends StatefulWidget {
  const SessionTerminal({super.key, required this.client, required this.sessionId, required this.active});
  final AshiraClient client;
  final String sessionId;

  /// Shown; focuses the current shell when it becomes visible.
  final bool active;

  @override
  State<SessionTerminal> createState() => _SessionTerminalState();
}

class _SessionTerminalState extends State<SessionTerminal> {
  /// The server allows 8 terminals at once across every session.
  static const _max = 8;
  var _next = 1;
  late final List<int> _tabs = [_next++];
  late int _current = _tabs.first;

  void _add() {
    if (_tabs.length >= _max) return;
    setState(() {
      _tabs.add(_next);
      _current = _next++;
    });
  }

  void _close(int id) {
    setState(() {
      final index = _tabs.indexOf(id);
      _tabs.removeAt(index);
      // The last tab closing opens a fresh shell instead of leaving the view empty.
      if (_tabs.isEmpty) _tabs.add(_next++);
      if (_current == id) _current = _tabs[index.clamp(0, _tabs.length - 1)];
    });
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return ColoredBox(
      color: _TerminalPaneState._background,
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        Container(
          height: 36,
          decoration: BoxDecoration(border: Border(bottom: BorderSide(color: scheme.border))),
          child: ListView(scrollDirection: Axis.horizontal, children: [
            for (final (index, id) in _tabs.indexed)
              _Tab(
                label: '终端 ${index + 1}',
                selected: id == _current,
                onSelect: () => setState(() => _current = id),
                onClose: () => _close(id),
              ),
            IconButton(
              tooltip: _tabs.length >= _max ? '最多 $_max 个终端' : '新建终端',
              iconSize: 18,
              visualDensity: VisualDensity.compact,
              icon: Icon(Icons.add, color: scheme.mutedForeground),
              onPressed: _tabs.length >= _max ? null : _add,
            ),
          ]),
        ),
        Expanded(
          child: IndexedStack(
            index: _tabs.indexOf(_current),
            sizing: StackFit.expand,
            children: [
              for (final id in _tabs)
                _TerminalPane(key: ValueKey(id), client: widget.client, sessionId: widget.sessionId, active: widget.active && id == _current),
            ],
          ),
        ),
      ]),
    );
  }
}

class _Tab extends StatelessWidget {
  const _Tab({required this.label, required this.selected, required this.onSelect, required this.onClose});
  final String label;
  final bool selected;
  final VoidCallback onSelect, onClose;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Material(
      color: selected ? scheme.surface.withValues(alpha: .4) : Colors.transparent,
      child: InkWell(
        onTap: onSelect,
        child: Container(
          padding: const EdgeInsets.only(left: 12, right: 4),
          decoration: BoxDecoration(border: Border(right: BorderSide(color: scheme.border))),
          child: Row(mainAxisSize: MainAxisSize.min, children: [
            Text(label, style: TextStyle(fontSize: 12, color: selected ? scheme.onSurface : scheme.mutedForeground)),
            IconButton(
              tooltip: '关闭$label',
              iconSize: 14,
              visualDensity: VisualDensity.compact,
              constraints: const BoxConstraints.tightFor(width: 28, height: 28),
              padding: EdgeInsets.zero,
              icon: Icon(Icons.close, color: scheme.mutedForeground),
              onPressed: onClose,
            ),
          ]),
        ),
      ),
    );
  }
}

/// One shell over `/v1/sessions/{id}/terminal`. The server opens it only with
/// `ASHIRA_WEB_TERMINAL=1`; a refused or dropped connection shows the web's explanation.
class _TerminalPane extends StatefulWidget {
  const _TerminalPane({super.key, required this.client, required this.sessionId, required this.active});
  final AshiraClient client;
  final String sessionId;
  final bool active;

  @override
  State<_TerminalPane> createState() => _TerminalPaneState();
}

enum _Link { connecting, open, closed }

class _TerminalPaneState extends State<_TerminalPane> {
  static const _background = Color(0xFF101012);
  static const _theme = TerminalTheme(
    cursor: Color(0xFFD4D4D8),
    selection: Color(0x66AEAFAD),
    foreground: Color(0xFFE4E4E7),
    background: _background,
    black: Color(0xFF000000),
    red: Color(0xFFCD3131),
    green: Color(0xFF0DBC79),
    yellow: Color(0xFFE5E510),
    blue: Color(0xFF2472C8),
    magenta: Color(0xFFBC3FBC),
    cyan: Color(0xFF11A8CD),
    white: Color(0xFFE5E5E5),
    brightBlack: Color(0xFF666666),
    brightRed: Color(0xFFF14C4C),
    brightGreen: Color(0xFF23D18B),
    brightYellow: Color(0xFFF5F543),
    brightBlue: Color(0xFF3B8EEA),
    brightMagenta: Color(0xFFD670D6),
    brightCyan: Color(0xFF29B8DB),
    brightWhite: Color(0xFFFFFFFF),
    searchHitBackground: Color(0xFFFFFF2B),
    searchHitBackgroundCurrent: Color(0xFF31FF26),
    searchHitForeground: Color(0xFF000000),
  );

  final _focus = FocusNode();
  late Terminal _terminal = _newTerminal();
  WebSocket? _socket;
  StreamSubscription<Object?>? _incoming;
  _Link _link = _Link.connecting;
  String _error = '';
  String _sizeSent = '';
  // PTY output arrives as raw bytes; a multi-byte character can straddle two frames.
  final _decoder = const Utf8Decoder(allowMalformed: true);
  ByteConversionSink? _utf8;

  Terminal _newTerminal() => Terminal(
        maxLines: 5000,
        onOutput: (data) {
          final socket = _socket;
          if (socket == null || _link != _Link.open) return;
          final bytes = utf8.encode(data);
          // The server reads at most 64 KiB per frame; stay well under it like the web.
          for (var start = 0; start < bytes.length; start += 16384) {
            socket.add(bytes.sublist(start, (start + 16384).clamp(0, bytes.length)));
          }
        },
        onResize: (cols, rows, _, _) => _sendSize(),
      );

  @override
  void initState() {
    super.initState();
    _connect();
  }

  @override
  void didUpdateWidget(_TerminalPane old) {
    super.didUpdateWidget(old);
    if (widget.active && !old.active && _link == _Link.open) _focus.requestFocus();
  }

  @override
  void dispose() {
    _close();
    _focus.dispose();
    super.dispose();
  }

  void _close() {
    _incoming?.cancel();
    _incoming = null;
    _socket?.close();
    _socket = null;
    _utf8?.close();
    _utf8 = null;
  }

  void _sendSize() {
    final socket = _socket;
    if (socket == null || _link != _Link.open) return;
    final size = jsonEncode({'cols': _terminal.viewWidth, 'rows': _terminal.viewHeight});
    if (size == _sizeSent) return;
    socket.add(size);
    _sizeSent = size;
  }

  Future<void> _connect() async {
    if (_socket != null) return;
    setState(() {
      // A fresh screen per connection, like the web's `terminal.reset()`.
      _terminal = _newTerminal();
      _link = _Link.connecting;
      _error = '';
      _sizeSent = '';
    });
    final base = Uri.parse(widget.client.baseUrl);
    final url = base.replace(
      scheme: base.scheme == 'https' ? 'wss' : 'ws',
      path: '/v1/sessions/${Uri.encodeComponent(widget.sessionId)}/terminal',
      queryParameters: widget.client.token.isEmpty ? null : {'token': widget.client.token},
    );
    try {
      final socket = await WebSocket.connect(url.toString());
      if (!mounted) {
        socket.close();
        return;
      }
      _socket = socket;
      final text = StringBuffer();
      _utf8 = _decoder.startChunkedConversion(_StringSink((chunk) => text.write(chunk)));
      _incoming = socket.listen(
        (data) {
          if (data is! List<int>) return;
          _utf8!.add(data);
          if (text.isEmpty) return;
          _terminal.write(text.toString());
          text.clear();
        },
        onDone: () => _closed(socket.closeReason),
        onError: (_) => _closed(null),
        cancelOnError: true,
      );
      setState(() => _link = _Link.open);
      _sendSize();
      if (widget.active) _focus.requestFocus();
    } catch (_) {
      if (mounted) _closed(null);
    }
  }

  void _closed(String? reason) {
    if (!mounted) return;
    final wasConnecting = _link == _Link.connecting;
    _close();
    setState(() {
      _error = reason != null && reason.isNotEmpty
          ? reason
          : wasConnecting
              ? '无法连接终端。请确认 API 启动时设置了 ASHIRA_WEB_TERMINAL=1。'
              : '终端连接已关闭。';
      _link = _Link.closed;
    });
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final mono = monoStyle(context);
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      if (_link != _Link.open)
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
          child: Row(children: [
            Expanded(child: Text(_link == _Link.connecting ? '连接终端中…' : _error, style: TextStyle(fontSize: 14, color: scheme.mutedForeground))),
            if (_link == _Link.closed)
              OutlinedButton(
                onPressed: _connect,
                style: OutlinedButton.styleFrom(minimumSize: const Size(0, 28), padding: const EdgeInsets.symmetric(horizontal: 10), textStyle: const TextStyle(fontSize: 12.8, fontWeight: FontWeight.w500)),
                child: const Text('重新连接'),
              ),
          ]),
        ),
      Expanded(
        child: TerminalView(
          _terminal,
          focusNode: _focus,
          theme: _theme,
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
          // Bundled CaskaydiaMono Nerd Font (pubspec `Ashira Terminal`), so prompt icons
          // (oh-my-posh, starship) render on every device, phones included.
          textStyle: TerminalStyle(
            fontSize: 13,
            height: 1.2,
            fontFamily: 'Ashira Terminal',
            fontFamilyFallback: [...?mono.fontFamilyFallback],
          ),
          cursorType: TerminalCursorType.block,
        ),
      ),
    ]);
  }
}

class _StringSink implements Sink<String> {
  _StringSink(this._onChunk);
  final void Function(String) _onChunk;
  @override
  void add(String data) => _onChunk(data);
  @override
  void close() {}
}
