import 'dart:async';
import 'dart:convert';
import 'dart:io' show WebSocket;

import 'package:flutter/material.dart';
import 'package:xterm/xterm.dart';

import '../api/client.dart';
import 'theme.dart';

/// The session's shell over `/v1/sessions/{id}/terminal`, like the web's TerminalView.
/// The server opens it only with `ASHIRA_WEB_TERMINAL=1`; a refused or dropped
/// connection shows the web's explanation.
class SessionTerminal extends StatefulWidget {
  const SessionTerminal({super.key, required this.client, required this.sessionId, required this.cwd, required this.active});
  final AshiraClient client;
  final String sessionId, cwd;

  /// Shown; focuses the shell when it becomes visible.
  final bool active;

  @override
  State<SessionTerminal> createState() => _SessionTerminalState();
}

enum _Link { connecting, open, closed }

class _SessionTerminalState extends State<SessionTerminal> {
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
  void didUpdateWidget(SessionTerminal old) {
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
    final small = TextStyle(fontSize: 12, color: scheme.mutedForeground);
    return ColoredBox(
      color: _background,
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        Container(
          height: 41,
          padding: const EdgeInsets.symmetric(horizontal: 16),
          decoration: BoxDecoration(border: Border(bottom: BorderSide(color: scheme.border))),
          child: Row(children: [
            Expanded(child: Tooltip(message: widget.cwd, child: Text(widget.cwd, maxLines: 1, overflow: TextOverflow.ellipsis, style: mono.copyWith(fontSize: 12, color: scheme.mutedForeground)))),
            if (_link == _Link.connecting) Padding(padding: const EdgeInsets.only(left: 12), child: Text('连接终端中…', style: small)),
            if (_link == _Link.closed)
              Padding(
                padding: const EdgeInsets.only(left: 12),
                child: OutlinedButton(
                  onPressed: _connect,
                  style: OutlinedButton.styleFrom(minimumSize: const Size(0, 28), padding: const EdgeInsets.symmetric(horizontal: 10), textStyle: const TextStyle(fontSize: 12.8, fontWeight: FontWeight.w500)),
                  child: const Text('重新打开终端'),
                ),
              ),
          ]),
        ),
        if (_error.isNotEmpty) Padding(padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8), child: Text(_error, style: TextStyle(fontSize: 14, color: scheme.mutedForeground))),
        Expanded(
          child: TerminalView(
            _terminal,
            focusNode: _focus,
            theme: _theme,
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
            textStyle: TerminalStyle(fontSize: 13, height: 1.2, fontFamily: mono.fontFamily ?? 'Cascadia Mono', fontFamilyFallback: mono.fontFamilyFallback ?? const []),
            cursorType: TerminalCursorType.block,
          ),
        ),
      ]),
    );
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
