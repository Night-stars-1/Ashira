import 'package:flutter/foundation.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Device-local display preferences: 设置 → 通用, and layout the App remembers.
class DisplayPrefs extends ChangeNotifier {
  static const _wrapKey = 'ashira.wrapToolOutput';
  static const _wrapTerminalKey = 'ashira.wrapTerminal';
  static const _sessionsKey = 'ashira.sessionsOpen';

  /// Long lines in tool output wrap; off keeps them whole and scrolls sideways.
  bool wrapToolOutput = true;

  /// The terminal tab wraps at its width; off gives it a wide fixed width that scrolls sideways.
  bool wrapTerminal = true;

  /// The session list beside the chat (wide layouts); folded stays folded across launches.
  bool sessionsOpen = true;

  Future<void> load() async {
    final prefs = await SharedPreferences.getInstance();
    wrapToolOutput = prefs.getBool(_wrapKey) ?? true;
    wrapTerminal = prefs.getBool(_wrapTerminalKey) ?? true;
    sessionsOpen = prefs.getBool(_sessionsKey) ?? true;
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

  Future<void> setSessionsOpen(bool value) async {
    sessionsOpen = value;
    notifyListeners();
    await (await SharedPreferences.getInstance()).setBool(_sessionsKey, value);
  }
}

final displayPrefs = DisplayPrefs();
