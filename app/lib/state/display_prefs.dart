import 'package:flutter/foundation.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Device-local display preferences, shown under 设置 → 显示.
class DisplayPrefs extends ChangeNotifier {
  static const _wrapKey = 'ashira.wrapToolOutput';

  /// Long lines in tool output wrap; off keeps them whole and scrolls sideways.
  bool wrapToolOutput = true;

  Future<void> load() async {
    wrapToolOutput = (await SharedPreferences.getInstance()).getBool(_wrapKey) ?? true;
    notifyListeners();
  }

  Future<void> setWrapToolOutput(bool value) async {
    wrapToolOutput = value;
    notifyListeners();
    await (await SharedPreferences.getInstance()).setBool(_wrapKey, value);
  }
}

final displayPrefs = DisplayPrefs();
