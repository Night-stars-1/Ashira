import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:markdown/markdown.dart' as md;
import 'package:url_launcher/url_launcher.dart';

import 'theme.dart';

/// Opens only http(s) and mailto links, like the web client's `safeHref`.
Future<void> openLink(String? href) async {
  final uri = href == null ? null : Uri.tryParse(href.trim());
  if (uri == null || !{'http', 'https', 'mailto'}.contains(uri.scheme)) return;
  await launchUrl(uri, mode: LaunchMode.externalApplication);
}

class Markdown extends StatelessWidget {
  const Markdown(this.text, {super.key});
  final String text;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final base = theme.textTheme.bodyMedium!.copyWith(height: 1.55);
    return MarkdownBody(
      data: text,
      selectable: true,
      onTapLink: (_, href, _) => openLink(href),
      // Remote images are not fetched; show the alt text like the web client.
      imageBuilder: (_, _, alt) => Text(alt ?? '', style: base.copyWith(color: scheme.onSurfaceVariant)),
      builders: {'pre': _CodeBlockBuilder()},
      styleSheet: MarkdownStyleSheet.fromTheme(theme).copyWith(
        p: base,
        strong: const TextStyle(fontWeight: FontWeight.w600),
        h1: base.copyWith(fontSize: 20, fontWeight: FontWeight.w600),
        h2: base.copyWith(fontSize: 18, fontWeight: FontWeight.w600),
        h3: base.copyWith(fontSize: 16, fontWeight: FontWeight.w600),
        h4: base.copyWith(fontWeight: FontWeight.w600),
        a: TextStyle(color: linkColor(context), decoration: TextDecoration.underline, decorationColor: linkColor(context)),
        code: monoStyle(context).copyWith(fontSize: 12, backgroundColor: scheme.muted),
        codeblockDecoration: BoxDecoration(color: scheme.card, borderRadius: BorderRadius.circular(14)),
        blockquoteDecoration: BoxDecoration(color: scheme.card, borderRadius: BorderRadius.circular(14), border: Border(left: BorderSide(color: scheme.border, width: 2))),
        blockquotePadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
        blockquote: base.copyWith(color: scheme.mutedForeground),
        tableHead: base.copyWith(fontWeight: FontWeight.w600),
        tableBody: base,
        tableHeadAlign: TextAlign.left,
        tableColumnWidth: const FlexColumnWidth(),
        tableBorder: TableBorder(horizontalInside: BorderSide(color: scheme.border), bottom: BorderSide(color: scheme.border)),
        tableCellsPadding: const EdgeInsets.all(8),
        horizontalRuleDecoration: BoxDecoration(border: Border(top: BorderSide(color: scheme.border))),
        blockSpacing: 12,
      ),
    );
  }
}

class _CodeBlockBuilder extends MarkdownElementBuilder {
  @override
  bool isBlockElement() => true;

  @override
  Widget? visitElementAfterWithContext(BuildContext context, md.Element element, TextStyle? preferredStyle, TextStyle? parentStyle) {
    final code = element.textContent.replaceFirst(RegExp(r'\n$'), '');
    final first = element.children?.firstOrNull;
    final language = first is md.Element ? (first.attributes['class'] ?? '').replaceFirst('language-', '') : '';
    return CodeBlock(code: code, language: language);
  }
}

class CodeBlock extends StatelessWidget {
  const CodeBlock({super.key, required this.code, this.language = ''});
  final String code, language;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 8, 8, 0),
          child: Row(children: [
            if (language.isNotEmpty)
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
                decoration: BoxDecoration(color: scheme.muted, borderRadius: BorderRadius.circular(999)),
                child: Text(language, style: const TextStyle(fontSize: 12, fontWeight: FontWeight.w500)),
              ),
            const Spacer(),
            IconButton(
              tooltip: '复制代码',
              icon: const Icon(Icons.copy_rounded, size: 14),
              style: IconButton.styleFrom(minimumSize: const Size.square(24), fixedSize: const Size.square(24)),
              onPressed: () async {
                await Clipboard.setData(ClipboardData(text: code));
                if (context.mounted) ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('已复制'), duration: Duration(seconds: 1)));
              },
            ),
          ]),
        ),
        SingleChildScrollView(
          scrollDirection: Axis.horizontal,
          padding: const EdgeInsets.fromLTRB(16, 8, 16, 12),
          child: SelectableText(code, style: monoStyle(context).copyWith(fontSize: 12, height: 1.6)),
        ),
      ],
    );
  }
}
