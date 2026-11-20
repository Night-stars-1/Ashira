import 'package:flutter/material.dart';
import 'package:re_editor/re_editor.dart';
import 'package:re_highlight/languages/bash.dart';
import 'package:re_highlight/languages/c.dart';
import 'package:re_highlight/languages/cpp.dart';
import 'package:re_highlight/languages/css.dart';
import 'package:re_highlight/languages/dart.dart';
import 'package:re_highlight/languages/go.dart';
import 'package:re_highlight/languages/ini.dart';
import 'package:re_highlight/languages/java.dart';
import 'package:re_highlight/languages/javascript.dart';
import 'package:re_highlight/languages/json.dart';
import 'package:re_highlight/languages/kotlin.dart';
import 'package:re_highlight/languages/markdown.dart';
import 'package:re_highlight/languages/powershell.dart';
import 'package:re_highlight/languages/python.dart';
import 'package:re_highlight/languages/rust.dart';
import 'package:re_highlight/languages/scss.dart';
import 'package:re_highlight/languages/sql.dart';
import 'package:re_highlight/languages/swift.dart';
import 'package:re_highlight/languages/typescript.dart';
import 'package:re_highlight/languages/vue.dart';
import 'package:re_highlight/languages/xml.dart';
import 'package:re_highlight/languages/yaml.dart';
import 'package:re_highlight/re_highlight.dart';
import 'package:re_highlight/styles/atom-one-dark.dart';
import 'package:re_highlight/styles/atom-one-light.dart';

import 'theme.dart';

/// Highlighting by extension, like the web's CodeMirror language table; anything else is plain text.
final Map<String, (String, Mode)> _languages = {
  for (final ext in ['rs']) ext: ('rust', langRust),
  for (final ext in ['dart']) ext: ('dart', langDart),
  for (final ext in ['js', 'mjs', 'cjs', 'jsx']) ext: ('javascript', langJavascript),
  for (final ext in ['ts', 'tsx', 'mts']) ext: ('typescript', langTypescript),
  for (final ext in ['vue']) ext: ('vue', langVue),
  for (final ext in ['json', 'jsonc', 'json5']) ext: ('json', langJson),
  for (final ext in ['yml', 'yaml']) ext: ('yaml', langYaml),
  for (final ext in ['toml', 'ini', 'cfg', 'conf']) ext: ('ini', langIni),
  for (final ext in ['md', 'markdown']) ext: ('markdown', langMarkdown),
  for (final ext in ['py', 'pyi']) ext: ('python', langPython),
  for (final ext in ['go']) ext: ('go', langGo),
  for (final ext in ['java']) ext: ('java', langJava),
  for (final ext in ['kt', 'kts']) ext: ('kotlin', langKotlin),
  for (final ext in ['swift']) ext: ('swift', langSwift),
  for (final ext in ['c', 'h']) ext: ('c', langC),
  for (final ext in ['cc', 'cpp', 'cxx', 'hpp', 'hh']) ext: ('cpp', langCpp),
  for (final ext in ['css']) ext: ('css', langCss),
  for (final ext in ['scss', 'sass', 'less']) ext: ('scss', langScss),
  for (final ext in ['html', 'htm', 'xml', 'svg']) ext: ('xml', langXml),
  for (final ext in ['sh', 'bash', 'zsh']) ext: ('bash', langBash),
  for (final ext in ['ps1', 'psm1']) ext: ('powershell', langPowershell),
  for (final ext in ['sql']) ext: ('sql', langSql),
};

/// A code editor with line numbers, folding, highlighting, search (Ctrl+F) and
/// undo, built on re_editor.
class CodeView extends StatelessWidget {
  const CodeView({super.key, required this.controller, required this.path});
  final CodeLineEditingController controller;
  final String path;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final dark = Theme.of(context).brightness == Brightness.dark;
    final name = path.split('/').last.toLowerCase();
    final language = _languages[name.contains('.') ? name.split('.').last : name];
    final mono = monoStyle(context);
    final gutter = TextStyle(fontSize: 12, color: scheme.mutedForeground);
    return CodeEditor(
      controller: controller,
      wordWrap: false,
      padding: const EdgeInsets.fromLTRB(4, 12, 12, 12),
      style: CodeEditorStyle(
        fontSize: 13,
        fontHeight: 1.5,
        fontFamily: mono.fontFamily,
        fontFamilyFallback: mono.fontFamilyFallback,
        textColor: scheme.onSurface,
        backgroundColor: scheme.surface,
        cursorColor: scheme.onSurface,
        cursorLineColor: scheme.muted.withValues(alpha: .5),
        selectionColor: scheme.outline.withValues(alpha: .4),
        codeTheme: language == null
            ? null
            : CodeHighlightTheme(languages: {language.$1: CodeHighlightThemeMode(mode: language.$2)}, theme: dark ? atomOneDarkTheme : atomOneLightTheme),
      ),
      indicatorBuilder: (context, editingController, chunkController, notifier) => Row(children: [
        DefaultCodeLineNumber(controller: editingController, notifier: notifier, textStyle: gutter, focusedTextStyle: gutter.copyWith(color: scheme.onSurface)),
        DefaultCodeChunkIndicator(width: 20, controller: chunkController, notifier: notifier),
      ]),
      toolbarController: const _ContextMenu(),
    );
  }
}

/// Right click / long press: cut, copy, paste, select all.
class _ContextMenu implements SelectionToolbarController {
  const _ContextMenu();

  @override
  void hide(BuildContext context) {}

  @override
  void show({
    required BuildContext context,
    required CodeLineEditingController controller,
    required TextSelectionToolbarAnchors anchors,
    Rect? renderRect,
    required LayerLink layerLink,
    required ValueNotifier<bool> visibility,
  }) {
    PopupMenuItem<void> item(String label, VoidCallback onTap) => PopupMenuItem(height: 32, onTap: onTap, child: Text(label, style: const TextStyle(fontSize: 13)));
    showMenu<void>(
      context: context,
      position: RelativeRect.fromSize(anchors.primaryAnchor & const Size(140, double.infinity), MediaQuery.sizeOf(context)),
      items: [
        item('剪切', controller.cut),
        item('复制', controller.copy),
        item('粘贴', controller.paste),
        item('全选', controller.selectAll),
      ],
    );
  }
}
