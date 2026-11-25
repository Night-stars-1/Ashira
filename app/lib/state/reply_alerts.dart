import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

import 'display_prefs.dart';

const _channel = MethodChannel('dev.ashira/replies');

/// Android: while replies run, one ongoing notification Android 16+ shows as a Live Update
/// (a status bar chip with the elapsed time, 「待回答」 while the model waits on the user);
/// when a reply ends or the model asks something, an alert. Both open their session.
///
/// Fed with which sessions run and ask ([sync]); the open session's own events add what
/// only they know ([finished], [asked]). No alert for the session on screen while the App
/// is in front.
class ReplyAlerts {
  ReplyAlerts({required this.onOpen}) {
    if (!supported) return;
    _lifecycle = AppLifecycleListener(onStateChange: (state) {
      _foreground = state == AppLifecycleState.resumed;
      // Back in the App on a session: its alert has been seen.
      if (_foreground && _shown.isNotEmpty) seen(_shown);
    });
    _channel.setMethodCallHandler((call) async {
      if (call.method == 'open' && call.arguments is String) onOpen(call.arguments as String);
    });
    _channel.invokeMethod('permission').catchError((Object _) {});
  }

  static bool get supported => !kIsWeb && Platform.isAndroid;

  /// The session a tapped notification opened the App on, once.
  static Future<String?> launchSession() async => supported ? _channel.invokeMethod<String>('launchSession').catchError((Object _) => null) : null;

  final void Function(String id) onOpen;
  AppLifecycleListener? _lifecycle;
  bool _foreground = true;
  bool _baseline = false;
  Set<String> _running = {};
  Set<String> _asking = {};
  /// When each running session was first seen running, for the elapsed time.
  final Map<String, int> _since = {};
  /// Ends and questions already alerted from the open session's events, so [sync] skips them.
  final Set<String> _told = {};
  String _shown = '';
  String _sent = '';

  bool _quiet(String id) => _foreground && id == _shown;

  /// The open session's reply ended; `error` `cancelled` is a stop the user asked for.
  void finished(String id, String title, {String? error, String answer = ''}) {
    if (!supported) return;
    _told.add('done:$id');
    if (error == 'cancelled' || _quiet(id)) return;
    _alert(id, title, error == null ? '回复完成' : '回复失败', error ?? _preview(answer), done: true);
  }

  /// The open session's model asks the user something.
  void asked(String id, String title, String question) {
    if (!supported) return;
    _told.add('ask:$id');
    if (_quiet(id)) return;
    _alert(id, title, '等待你回答', _preview(question), done: false);
  }

  /// The user opened this session: its alert has been seen.
  void seen(String id) {
    if (supported) _channel.invokeMethod('cancel', {'id': id}).catchError((Object _) {});
  }

  void sync({required Set<String> running, required Set<String> asking, required String shown, required String Function(String id) title}) {
    if (!supported) return;
    _shown = shown;
    final now = DateTime.now().millisecondsSinceEpoch;
    if (_baseline) {
      for (final id in _running.difference(running)) {
        if (!_told.contains('done:$id') && !_quiet(id)) _alert(id, title(id), '回复已结束', '', done: true);
      }
      for (final id in asking.difference(_asking)) {
        if (!_told.contains('ask:$id') && !_quiet(id)) _alert(id, title(id), '等待你回答', '', done: false);
      }
      // Answered elsewhere (the web, another device) while the reply goes on.
      for (final id in _asking.difference(asking)) {
        if (running.contains(id)) seen(id);
      }
    }
    _baseline = true;
    // An event's alert stands in for the list change it causes, once that change is seen.
    _told.removeWhere((key) {
      final id = key.substring(4);
      return key.startsWith('done:') ? !running.contains(id) : asking.contains(id) || !running.contains(id);
    });
    _running = {...running};
    _asking = {...asking};
    _since.removeWhere((id, _) => !running.contains(id));
    for (final id in running) {
      _since.putIfAbsent(id, () => now);
    }
    final sessions = [
      for (final id in running) {'id': id, 'title': title(id), 'asking': asking.contains(id), 'since': _since[id]},
    ]..sort((a, b) => (a['since'] as int).compareTo(b['since'] as int));
    final key = '${displayPrefs.customIsland}|${sessions.map((s) => '${s['id']}|${s['title']}|${s['asking']}').join(',')}';
    if (key == _sent) return;
    _sent = key;
    _channel.invokeMethod('update', {'sessions': sessions, 'customIsland': displayPrefs.customIsland}).catchError((Object _) {});
  }

  void dispose() {
    if (!supported) return;
    _lifecycle?.dispose();
    _channel.setMethodCallHandler(null);
    _channel.invokeMethod('update', {'sessions': <Object>[]}).catchError((Object _) {});
  }

  /// `done`: the reply ended; else it asks something. Where the island shows, an end shows
  /// on it for a moment instead of popping up, and a question as its 「待回答」.
  void _alert(String id, String title, String text, String detail, {required bool done}) {
    final chip = text == '回复失败' ? '失败' : '已完成';
    _channel.invokeMethod('alert', {'id': id, 'title': title, 'text': text, 'detail': detail, 'done': done, 'chip': chip, 'customIsland': displayPrefs.customIsland}).catchError((Object _) {});
  }

  static String _preview(String text) {
    final line = text.replaceAll(RegExp(r'\s+'), ' ').trim();
    return line.length > 160 ? '${line.substring(0, 160)}…' : line;
  }
}
