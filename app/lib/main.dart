import 'package:flutter/material.dart';

import 'state/app_update.dart';
import 'state/display_prefs.dart';
import 'state/ashira_state.dart';
import 'state/settings.dart';
import 'ui/connect_page.dart';
import 'ui/home_page.dart';
import 'ui/theme.dart';
import 'ui/window_frame.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await setupWindow();
  await displayPrefs.load();
  runApp(const AshiraApp());
  appUpdate.check(quiet: true);
}

class AshiraApp extends StatefulWidget {
  const AshiraApp({super.key});
  @override
  State<AshiraApp> createState() => _AshiraAppState();
}

class _AshiraAppState extends State<AshiraApp> {
  ConnectionSettings? _saved;
  AshiraState? _state;
  String? _message;
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    ConnectionSettings.load().then((saved) {
      if (!mounted) return;
      _saved = saved;
      if (saved != null) _attach(saved);
      setState(() => _loading = false);
    });
  }

  void _attach(ConnectionSettings settings) {
    _state?.dispose();
    _saved = settings;
    _message = null;
    _state = AshiraState(settings)..start();
  }

  void _disconnect() {
    final state = _state;
    if (state == null) return;
    setState(() {
      _message = state.unauthorized ? 'API Token 不正确或缺失' : null;
      _state = null;
    });
    state.dispose();
  }

  @override
  void dispose() {
    _state?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final state = _state;
    return MaterialApp(
      title: 'Ashira',
      debugShowCheckedModeBanner: false,
      theme: buildTheme(Brightness.light),
      darkTheme: buildTheme(Brightness.dark),
      builder: (_, child) => WindowResizeEdges(child: child!),
      home: _loading
          ? const Scaffold(body: Column(children: [WindowBar(), Expanded(child: Center(child: CircularProgressIndicator()))]))
          : state == null
              ? ConnectPage(initial: _saved, message: _message, onConnected: (settings) => setState(() => _attach(settings)))
              : HomePage(key: ObjectKey(state), state: state, onDisconnect: _disconnect),
    );
  }
}
