import 'dart:convert';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:image_picker/image_picker.dart';

import '../api/models.dart';
import 'context_ring.dart';
import 'question_form.dart';
import 'text_file_card.dart';
import 'theme.dart';

typedef OutgoingImage = ({String mediaType, String data});

const _maxImages = 4;
const _well = OutlineInputBorder(borderRadius: BorderRadius.all(Radius.circular(10)), borderSide: BorderSide.none);
const _maxImageBytes = 1500000; // 2,000,000 base64 characters in the API.
const _maxTextBytes = 8 * 1024 * 1024; // the API's request body limit.
const _types = {'png': 'image/png', 'jpg': 'image/jpeg', 'jpeg': 'image/jpeg', 'gif': 'image/gif', 'webp': 'image/webp'};

/// A paste this long becomes an attached file instead of flooding the input.
const _pasteLines = 12;
const _pasteChars = 1200;

class _Attachment {
  _Attachment(this.name, this.mediaType, this.bytes);
  final String name, mediaType;
  final Uint8List bytes;
}

class Composer extends StatefulWidget {
  const Composer({
    super.key,
    required this.enabled,
    required this.hasSession,
    required this.busy,
    required this.model,
    this.context = 0,
    this.window,
    required this.onSend,
    required this.onInterrupt,
    required this.onModels,
    required this.onAnswer,
    this.question,
    this.error = '',
  });

  final bool enabled, busy;
  /// Picks the hint; [enabled] also drops while the session's stream reconnects.
  final bool hasSession;
  final String model, error;
  /// Context tokens in use and the model's window, drawn as a ring by the send button.
  final int context;
  final int? window;
  final Future<bool> Function(String text, List<OutgoingImage> images, List<TextFile> files) onSend;
  final VoidCallback onInterrupt, onModels;
  /// Pending `ask_user`; while set, the card shows the question form and the
  /// task draft waits untouched underneath.
  final QuestionRequest? question;
  final Future<void> Function(Map<String, Object?>? answers) onAnswer;

  @override
  State<Composer> createState() => _ComposerState();
}

class _ComposerState extends State<Composer> {
  final _text = TextEditingController();
  final _focus = FocusNode();
  final _images = <_Attachment>[];
  final _files = <TextFile>[];
  String _attachError = '';
  bool _sending = false;
  int _pasteCount = 0;

  @override
  void initState() {
    super.initState();
    _text.addListener(() => setState(() {}));
  }

  @override
  void dispose() {
    _text.dispose();
    _focus.dispose();
    super.dispose();
  }

  bool get _canSend => widget.enabled && !widget.busy && !_sending && (_text.text.trim().isNotEmpty || _images.isNotEmpty || _files.isNotEmpty);

  Future<void> _send() async {
    if (!_canSend) return;
    final text = _text.text;
    final attached = List.of(_images);
    final files = List.of(_files);
    setState(() => _sending = true);
    final ok = await widget.onSend(text, [for (final image in attached) (mediaType: image.mediaType, data: base64Encode(image.bytes))], files);
    if (!mounted) return;
    setState(() {
      _sending = false;
      if (ok) {
        if (_text.text == text) _text.clear();
        _images.removeWhere(attached.contains);
        _files.removeWhere(files.contains);
      }
    });
  }

  /// Phones pick photos from the gallery.
  Future<void> _pickImages() async {
    setState(() => _attachError = '');
    final List<XFile> files;
    try {
      files = await ImagePicker().pickMultiImage(limit: _maxImages - _images.length);
    } on PlatformException catch (error) {
      setState(() => _attachError = '无法打开图片：${error.message ?? error.code}');
      return;
    }
    for (final file in files) {
      _addImage(file.name, await file.readAsBytes());
    }
    if (mounted) setState(() {});
  }

  /// Desktop picks images and text files together; phones use it for text files.
  Future<void> _pickFiles() async {
    setState(() => _attachError = '');
    final List<PlatformFile> picked;
    try {
      picked = await FilePicker.pickFiles();
    } on PlatformException catch (error) {
      setState(() => _attachError = '无法打开文件：${error.message ?? error.code}');
      return;
    }
    for (final file in picked) {
      final image = _types.containsKey((file.extension ?? '').replaceFirst('.', '').toLowerCase());
      // Check the size before reading so an oversized file is never loaded.
      final length = await file.length();
      if (length != null && length > (image ? _maxImageBytes : _maxTextBytes)) {
        _attachError = image ? '「${file.name}」超过单张 1.5 MB 的限制。' : '「${file.name}」超过 8 MB 的请求上限。';
        continue;
      }
      final bytes = await file.readAsBytes();
      if (image) {
        _addImage(file.name, bytes);
      } else {
        _addText(file.name, bytes);
      }
    }
    if (mounted) setState(() {});
  }

  void _addImage(String name, Uint8List bytes) {
    final extension = name.contains('.') ? name.split('.').last.toLowerCase() : '';
    final type = _types[extension];
    if (_images.length >= _maxImages) {
      _attachError = '最多添加 $_maxImages 张图片。';
    } else if (type == null) {
      _attachError = '「$name」格式不支持，请选择 PNG、JPEG、GIF 或 WebP。';
    } else if (bytes.length > _maxImageBytes) {
      _attachError = '「$name」超过单张 1.5 MB 的限制。';
    } else {
      _images.add(_Attachment(name, type, bytes));
    }
  }

  void _addText(String name, Uint8List bytes) {
    if (bytes.length > _maxTextBytes) {
      _attachError = '「$name」超过 8 MB 的请求上限。';
      return;
    }
    // NUL bytes mean a binary file; the model would only see mojibake.
    if (bytes.take(8192).contains(0)) {
      _attachError = '「$name」不是文本文件，只能添加文本或图片。';
      return;
    }
    final text = utf8.decode(bytes, allowMalformed: true).replaceAll(RegExp(r'\r\n?'), '\n');
    if (text.trim().isEmpty) {
      _attachError = '「$name」是空文件。';
      return;
    }
    _files.add(TextFile.of(name, text));
  }

  /// Long pasted text goes in as a file card, like the web; short text pastes normally.
  Future<void> _paste() async {
    final text = (await Clipboard.getData(Clipboard.kTextPlain))?.text ?? '';
    if (text.isEmpty || !mounted) return;
    if (text.length > _pasteChars || '\n'.allMatches(text).length >= _pasteLines) {
      setState(() => _files.add(TextFile.of('粘贴文本 ${++_pasteCount}.txt', text.replaceAll(RegExp(r'\r\n?'), '\n'))));
      return;
    }
    final value = _text.value;
    final start = value.selection.isValid ? value.selection.start : value.text.length;
    final end = value.selection.isValid ? value.selection.end : value.text.length;
    _text.value = TextEditingValue(text: value.text.replaceRange(start, end, text), selection: TextSelection.collapsed(offset: start + text.length));
  }

  /// Desktop: Enter sends, Shift+Enter inserts a newline. Phones use the send button.
  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent || event.logicalKey != LogicalKeyboardKey.enter) return KeyEventResult.ignored;
    if (HardwareKeyboard.instance.isShiftPressed || _text.value.composing.isValid) return KeyEventResult.ignored;
    _send();
    return KeyEventResult.handled;
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final desktop = isDesktop;
    final wide = MediaQuery.sizeOf(context).width >= 640;
    final question = widget.question;
    return SafeArea(
      top: false,
      child: Padding(
        padding: EdgeInsets.fromLTRB(wide ? 24 : 8, 0, wide ? 24 : 8, wide ? 24 : 8),
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 768),
            child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.stretch, children: [
              if (widget.error.isNotEmpty) Padding(padding: const EdgeInsets.fromLTRB(4, 0, 4, 8), child: Text(widget.error, style: TextStyle(color: scheme.error, fontSize: 13))),
              DecoratedBox(
                decoration: BoxDecoration(
                  color: scheme.card,
                  borderRadius: BorderRadius.circular(18),
                  border: Border.all(color: question != null ? scheme.primary.withValues(alpha: .6) : scheme.border),
                  boxShadow: [BoxShadow(color: Colors.black.withValues(alpha: .08), blurRadius: 2, offset: const Offset(0, 1))],
                ),
                child: Padding(
                  padding: const EdgeInsets.all(8),
                  child: question != null
                      ? QuestionForm(request: question, onAnswer: widget.onAnswer, onInterrupt: widget.onInterrupt)
                      : Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                    if (_images.isNotEmpty || _files.isNotEmpty)
                      SizedBox(
                        height: _files.isEmpty ? 72 : 92,
                        child: ListView(scrollDirection: Axis.horizontal, children: [
                          for (final (index, image) in _images.indexed)
                            Padding(
                              padding: const EdgeInsets.only(right: 8, top: 4),
                              child: Stack(children: [
                                ClipRRect(borderRadius: BorderRadius.circular(8), child: Image.memory(image.bytes, width: 88, height: 64, fit: BoxFit.cover)),
                                Positioned(
                                  right: 2,
                                  top: 2,
                                  child: InkWell(
                                    onTap: () => setState(() => _images.removeAt(index)),
                                    child: CircleAvatar(radius: 11, backgroundColor: scheme.surface, child: Icon(Icons.close, size: 14, semanticLabel: '移除图片 ${index + 1}')),
                                  ),
                                ),
                              ]),
                            ),
                          for (final file in _files)
                            Padding(
                              padding: const EdgeInsets.only(right: 8, top: 4),
                              child: Align(alignment: Alignment.topLeft, child: TextFileCard(file: file, onRemove: () => setState(() => _files.remove(file)))),
                            ),
                        ]),
                      ),
                    if (_attachError.isNotEmpty) Padding(padding: const EdgeInsets.all(4), child: Text(_attachError, style: TextStyle(color: scheme.error, fontSize: 12))),
                    Focus(
                      onKeyEvent: desktop ? _onKey : null,
                      child: Actions(
                        // Long pastes become file cards; Ctrl+V and the context menu both land here.
                        actions: {PasteTextIntent: CallbackAction<PasteTextIntent>(onInvoke: (_) => _paste())},
                        child: TextField(
                        controller: _text,
                        focusNode: _focus,
                        enabled: widget.enabled && !widget.busy && !_sending,
                        minLines: 2,
                        maxLines: 8,
                        keyboardType: TextInputType.multiline,
                        textInputAction: TextInputAction.newline,
                        style: const TextStyle(fontSize: 14, height: 1.43),
                        decoration: InputDecoration(
                          border: _well,
                          enabledBorder: _well,
                          focusedBorder: _well,
                          disabledBorder: _well,
                          filled: true,
                          fillColor: Colors.white.withValues(alpha: Theme.of(context).brightness == Brightness.dark ? .045 : 0),
                          isDense: true,
                          contentPadding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
                          // Like the web: only a missing session changes the hint; the brief reconnect
                          // while switching sessions keeps it steady.
                          hintText: widget.hasSession ? (desktop ? '描述任务，Enter 发送，Shift+Enter 换行' : '描述任务') : '请先选择或新建会话',
                        ),
                        ),
                      ),
                    ),
                    const SizedBox(height: 6),
                    Row(children: [
                      if (desktop)
                        IconButton(tooltip: '添加图片（单张不超过 1.5 MB）或文本文件', icon: const Icon(Icons.attach_file), onPressed: widget.enabled && !widget.busy ? _pickFiles : null)
                      else ...[
                        IconButton(tooltip: '添加图片（单张不超过 1.5 MB）', icon: const Icon(Icons.add_photo_alternate_outlined), onPressed: widget.enabled && !widget.busy && _images.length < _maxImages ? _pickImages : null),
                        IconButton(tooltip: '添加文本文件', icon: const Icon(Icons.attach_file), onPressed: widget.enabled && !widget.busy ? _pickFiles : null),
                      ],
                      const SizedBox(width: 4),
                      // Takes the leftover width so the send button sits at the right edge.
                      Expanded(
                        child: Align(
                          alignment: Alignment.centerLeft,
                          child: ConstrainedBox(
                            constraints: const BoxConstraints(maxWidth: 288),
                            child: OutlinedButton(
                              onPressed: widget.enabled && !widget.busy ? widget.onModels : null,
                              child: Row(mainAxisSize: MainAxisSize.min, children: [
                                Flexible(child: Text(widget.model.isEmpty ? '选择模型' : widget.model, maxLines: 1, overflow: TextOverflow.ellipsis)),
                                const SizedBox(width: 4),
                                const Icon(Icons.expand_more, size: 16),
                              ]),
                            ),
                          ),
                        ),
                      ),
                      if (widget.hasSession && widget.context > 0) ...[
                        const SizedBox(width: 4),
                        ContextRing(context: widget.context, window: widget.window),
                      ],
                      const SizedBox(width: 8),
                      if (widget.busy)
                        FilledButton.icon(
                          style: FilledButton.styleFrom(backgroundColor: scheme.error, foregroundColor: scheme.onError),
                          onPressed: widget.onInterrupt,
                          icon: const Icon(Icons.stop_rounded),
                          label: const Text('停止'),
                        )
                      else
                        FilledButton.icon(onPressed: _canSend ? _send : null, icon: const Icon(Icons.arrow_upward_rounded), label: const Text('发送')),
                    ]),
                  ]),
                ),
              ),
            ]),
          ),
        ),
      ),
    );
  }
}
