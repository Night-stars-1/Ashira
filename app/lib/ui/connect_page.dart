import 'package:flutter/material.dart';

import '../api/client.dart';
import '../state/settings.dart';
import 'theme.dart';
import 'window_frame.dart';

class ConnectPage extends StatefulWidget {
  const ConnectPage({super.key, this.initial, this.message, required this.onConnected});
  final ConnectionSettings? initial;
  final String? message;
  final void Function(ConnectionSettings settings) onConnected;

  @override
  State<ConnectPage> createState() => _ConnectPageState();
}

class _ConnectPageState extends State<ConnectPage> {
  late final _url = TextEditingController(text: widget.initial?.baseUrl ?? '');
  late final _token = TextEditingController(text: widget.initial?.token ?? '');
  late String _error = widget.message ?? '';
  bool _testing = false;
  bool _showToken = false;

  @override
  void dispose() {
    _url.dispose();
    _token.dispose();
    super.dispose();
  }

  Future<void> _connect() async {
    final url = ConnectionSettings.normalize(_url.text);
    if (url.isEmpty) {
      setState(() => _error = '请输入服务器地址，例如 192.168.1.5:8787');
      return;
    }
    final settings = ConnectionSettings(baseUrl: url, token: _token.text.trim());
    setState(() {
      _testing = true;
      _error = '';
    });
    final client = AshiraClient(baseUrl: settings.baseUrl, token: settings.token);
    try {
      await client.status();
      await settings.save();
      widget.onConnected(settings);
    } on ApiException catch (error) {
      if (mounted) setState(() => _error = error.unauthorized ? 'API Token 不正确或缺失' : error.message);
    } finally {
      client.close();
      if (mounted) setState(() => _testing = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final muted = TextStyle(fontSize: 13, color: scheme.mutedForeground);
    Widget label(String text) => Padding(
      padding: const EdgeInsets.only(bottom: 6),
      child: Text(text, style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w500)),
    );
    return Scaffold(
      body: SafeArea(
        child: Column(
          children: [
            const WindowBar(),
            Expanded(
              child: Center(
                child: SingleChildScrollView(
                  padding: const EdgeInsets.all(24),
                  child: ConstrainedBox(
                    constraints: const BoxConstraints(maxWidth: 380),
                    child: AutofillGroup(
                      child: Container(
                        padding: const EdgeInsets.all(24),
                        decoration: BoxDecoration(
                          color: scheme.card,
                          border: Border.all(color: scheme.border),
                          borderRadius: BorderRadius.circular(12),
                        ),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            const Text('连接 Ashira', style: TextStyle(fontSize: 18, fontWeight: FontWeight.w600)),
                            const SizedBox(height: 6),
                            Text('连接到运行 ashira --serve 的电脑。', style: muted),
                            const SizedBox(height: 24),
                            label('服务器地址'),
                            TextField(
                              controller: _url,
                              keyboardType: TextInputType.url,
                              autofillHints: const [AutofillHints.url],
                              decoration: const InputDecoration(hintText: '192.168.1.5:8787'),
                              onSubmitted: (_) => _connect(),
                            ),
                            const SizedBox(height: 16),
                            label('API Token'),
                            TextField(
                              controller: _token,
                              obscureText: !_showToken,
                              autofillHints: const [AutofillHints.password],
                              decoration: InputDecoration(
                                hintText: '与服务端 ASHIRA_API_TOKEN 相同',
                                suffixIcon: IconButton(
                                  tooltip: _showToken ? '隐藏' : '显示',
                                  icon: Icon(_showToken ? Icons.visibility_off_outlined : Icons.visibility_outlined),
                                  onPressed: () => setState(() => _showToken = !_showToken),
                                ),
                              ),
                              onSubmitted: (_) => _connect(),
                            ),
                            if (_error.isNotEmpty)
                              Padding(
                                padding: const EdgeInsets.only(top: 12),
                                child: Text(_error, style: TextStyle(fontSize: 13, color: scheme.error)),
                              ),
                            const SizedBox(height: 20),
                            FilledButton(
                              onPressed: _testing ? null : _connect,
                              child: _testing ? SizedBox.square(dimension: 16, child: CircularProgressIndicator(strokeWidth: 2, color: scheme.onPrimary)) : const Text('连接'),
                            ),
                            const SizedBox(height: 20),
                            Divider(color: scheme.border),
                            const SizedBox(height: 12),
                            Text('在电脑上运行，手机和电脑需在同一局域网：', style: muted.copyWith(fontSize: 12)),
                            const SizedBox(height: 6),
                            Container(
                              padding: const EdgeInsets.all(10),
                              decoration: BoxDecoration(color: scheme.muted, borderRadius: BorderRadius.circular(6)),
                              child: SelectableText('ASHIRA_API_TOKEN=<随机字符串> ashira --listen 0.0.0.0:8787', style: monoStyle(context).copyWith(fontSize: 11.5, height: 1.5)),
                            ),
                          ],
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
