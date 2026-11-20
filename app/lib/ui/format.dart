String tokens(num value) {
  final text = value.round().toString();
  final out = StringBuffer();
  for (var i = 0; i < text.length; i++) {
    if (i > 0 && (text.length - i) % 3 == 0) out.write(',');
    out.write(text[i]);
  }
  return out.toString();
}

String compact(num value) {
  if (value >= 1e6) return '${_trim(value / 1e6)}m';
  if (value >= 1e3) return '${_trim(value / 1e3)}k';
  return value.round().toString();
}

/// Token counts in Chinese units (`7.5万`, `1.2亿`), matching the web's `zh-CN` compact format.
String wan(num value) {
  if (value >= 1e8) return '${_trim(value / 1e8)}亿';
  if (value >= 1e4) return '${_trim(value / 1e4)}万';
  return value.round().toString();
}

String _trim(num value) => value.toStringAsFixed(1).replaceFirst(RegExp(r'\.0$'), '');

String dollars(double value) {
  if (value == 0) return r'$0.00';
  final digits = value >= 1 ? 2 : value >= 0.01 ? 4 : 6;
  // Keep at least two decimals, drop trailing zeros beyond that.
  final fixed = value.toStringAsFixed(digits).replaceAllMapped(RegExp(r'(\.\d\d\d*?)0+$'), (match) => match[1]!);
  return '\$$fixed';
}

String ago(int millis) {
  if (millis <= 0) return '';
  final diff = DateTime.now().difference(DateTime.fromMillisecondsSinceEpoch(millis));
  if (diff.inMinutes < 1) return '刚刚';
  if (diff.inMinutes < 60) return '${diff.inMinutes}分钟前';
  if (diff.inHours < 24) return '${diff.inHours}小时前';
  if (diff.inDays == 1) return '昨天';
  if (diff.inDays == 2) return '前天';
  if (diff.inDays < 30) return '${diff.inDays}天前';
  final date = DateTime.fromMillisecondsSinceEpoch(millis);
  return '${date.year}/${date.month}/${date.day}';
}

String fullTime(int millis) {
  final t = DateTime.fromMillisecondsSinceEpoch(millis);
  String two(int n) => n.toString().padLeft(2, '0');
  return '${t.year}-${two(t.month)}-${two(t.day)} ${two(t.hour)}:${two(t.minute)}';
}
