import 'package:shared_preferences/shared_preferences.dart';

/// Server address and token, persisted on the device.
class ConnectionSettings {
  const ConnectionSettings({required this.baseUrl, this.token = ''});

  final String baseUrl;
  final String token;

  static const _urlKey = 'ashira.baseUrl';
  static const _tokenKey = 'ashira.token';
  static const _sessionKey = 'ashira.session';

  static Future<ConnectionSettings?> load() async {
    final prefs = await SharedPreferences.getInstance();
    final url = prefs.getString(_urlKey);
    if (url == null || url.isEmpty) return null;
    return ConnectionSettings(baseUrl: url, token: prefs.getString(_tokenKey) ?? '');
  }

  Future<void> save() async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(_urlKey, baseUrl);
    await prefs.setString(_tokenKey, token);
  }

  static Future<String?> lastSession() async => (await SharedPreferences.getInstance()).getString(_sessionKey);

  static Future<void> rememberSession(String id) async => (await SharedPreferences.getInstance()).setString(_sessionKey, id);

  /// Accepts `host`, `host:port` or a full URL; defaults to http and port 8787.
  static String normalize(String input) {
    // Chinese IMEs often produce full-width ：。／ while typing an address.
    var value = input.trim().replaceAll('：', ':').replaceAll('。', '.').replaceAll('／', '/').replaceAll(' ', '');
    if (value.isEmpty) return '';
    if (!value.contains('://')) value = 'http://$value';
    final uri = Uri.tryParse(value);
    if (uri == null || !{'http', 'https'}.contains(uri.scheme) || !RegExp(r'^[A-Za-z0-9.\-]+$|^\[[0-9A-Fa-f:]+\]$').hasMatch(uri.host)) return '';
    final port = uri.hasPort ? uri.port : 8787;
    return Uri(scheme: uri.scheme, host: uri.host, port: port).toString();
  }
}
