import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// The 逐回复明细 table's order: a column key and whether it runs ascending.
typedef ReplySort = ({String key, bool ascending});

const replySortKeys = ['number', 'input', 'output', 'cache', 'hit', 'rate', 'firstOutputMs', 'firstTextMs', 'cost'];
const defaultReplySort = (key: 'number', ascending: true);

/// Device-local display preferences: 设置 → 通用, and layout the App remembers.
class DisplayPrefs extends ChangeNotifier {
  static const _wrapKey = 'ashira.wrapToolOutput';
  static const _foldKey = 'ashira.foldToolOutput';
  static const _wrapTerminalKey = 'ashira.wrapTerminal';
  static const _sessionsKey = 'ashira.sessionsOpen';
  static const _replySortKey = 'ashira.replySort';

  /// Long lines in tool output wrap; off keeps them whole and scrolls sideways.
  bool wrapToolOutput = true;

  /// Finished tool cards show only their header until opened; a running or failed tool and
  /// the todo board stay open.
  bool foldToolOutput = false;

  /// The terminal tab wraps at its width; off gives it a wide fixed width that scrolls sideways.
  bool wrapTerminal = true;

  /// The session list beside the chat (wide layouts); folded stays folded across launches.
  bool sessionsOpen = true;

  /// The 逐回复明细 table's order; kept on this device only.
  ReplySort replySort = defaultReplySort;

  Future<void> load() async {
    final prefs = await SharedPreferences.getInstance();
    wrapToolOutput = prefs.getBool(_wrapKey) ?? true;
    foldToolOutput = prefs.getBool(_foldKey) ?? false;
    wrapTerminal = prefs.getBool(_wrapTerminalKey) ?? true;
    sessionsOpen = prefs.getBool(_sessionsKey) ?? true;
    replySort = _parseSort(prefs.getString(_replySortKey));
    notifyListeners();
  }

  Future<void> setWrapTerminal(bool value) async {
    wrapTerminal = value;
    notifyListeners();
    await (await SharedPreferences.getInstance()).setBool(_wrapTerminalKey, value);
  }

  Future<void> setWrapToolOutput(bool value) async {
    wrapToolOutput = value;
    notifyListeners();
    await (await SharedPreferences.getInstance()).setBool(_wrapKey, value);
  }

  Future<void> setFoldToolOutput(bool value) async {
    foldToolOutput = value;
    notifyListeners();
    await (await SharedPreferences.getInstance()).setBool(_foldKey, value);
  }

  Future<void> setSessionsOpen(bool value) async {
    sessionsOpen = value;
    notifyListeners();
    await (await SharedPreferences.getInstance()).setBool(_sessionsKey, value);
  }

  Future<void> setReplySort(ReplySort value) async {
    replySort = value;
    notifyListeners();
    await (await SharedPreferences.getInstance()).setString(_replySortKey, jsonEncode({'key': value.key, 'dir': value.ascending ? 'asc' : 'desc'}));
  }

  /// The same JSON the web keeps under this key; anything unreadable is the default order.
  static ReplySort _parseSort(String? raw) {
    if (raw == null) return defaultReplySort;
    try {
      final value = jsonDecode(raw);
      if (value is Map && replySortKeys.contains(value['key']) && (value['dir'] == 'asc' || value['dir'] == 'desc')) {
        return (key: value['key'] as String, ascending: value['dir'] == 'asc');
      }
    } on FormatException {
      // Fall through to the default.
    }
    return defaultReplySort;
  }
}

final displayPrefs = DisplayPrefs();
