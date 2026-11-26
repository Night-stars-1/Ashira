import 'dart:io';
import 'dart:ui' as ui;

import 'package:ashira_app/state/ashira_state.dart';
import 'package:ashira_app/state/settings.dart';
import 'package:ashira_app/ui/home_page.dart';
import 'package:ashira_app/ui/theme.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';

void main() {
  testWidgets('start page against a live server', (tester) async {
    SharedPreferences.setMockInitialValues({});
    HttpOverrides.global = null;
    tester.view.physicalSize = const Size(1200, 760);
    tester.view.devicePixelRatio = 1;
    late AshiraState state;
    await tester.runAsync(() async {
      state = AshiraState(const ConnectionSettings(baseUrl: 'http://127.0.0.1:8818', token: 'starttest'));
      await state.startNew(r'C:\Users\nujj2\AppData\Local\Temp\start-e2e\work');
    });
    final key = GlobalKey();
    await tester.pumpWidget(MaterialApp(
      theme: buildTheme(Brightness.dark),
      home: RepaintBoundary(key: key, child: HomePage(state: state, onDisconnect: () {})),
    ));
    await tester.pump(const Duration(milliseconds: 300));
    expect(find.text('要做什么？'), findsOneWidget);
    debugPrint('error=${state.error} model=${state.modelTitle} approval=${state.status?.approvalMode} cwd=${state.cwd} canApprove=${state.canSetApprovalMode}');
    await tester.runAsync(() async {
      final boundary = key.currentContext!.findRenderObject()! as RenderRepaintBoundary;
      final image = await boundary.toImage();
      final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
      File('${Platform.environment['LOCALAPPDATA']}/Temp/start-app.png').writeAsBytesSync(bytes!.buffer.asUint8List());
    });
  });
}
