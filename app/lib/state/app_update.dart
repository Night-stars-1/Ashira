import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:http/http.dart' as http;
import 'package:package_info_plus/package_info_plus.dart';
import 'package:path_provider/path_provider.dart';

const _repo = 'Night-stars-1/Ashira';
const _channel = MethodChannel('dev.ashira/update');

/// The App's one updater: checked at start-up, shown in the sidebar and settings.
final appUpdate = AppUpdate();

/// A newer App release on GitHub and the asset this platform installs.
class AppRelease {
  const AppRelease({required this.tag, required this.page, required this.asset, required this.size});
  final String tag, page, asset;
  final int size;
}

/// In-app update: checks GitHub Releases for a newer App build, downloads it, and installs
/// it only when asked. Android hands the APK to the system installer; on the desktops a
/// script waits for the App to quit, puts the new build in place (Windows: the installer,
/// silently) and starts it again.
class AppUpdate extends ChangeNotifier {
  String current = '';
  /// The latest release tag, once a check succeeded (newer or not).
  String? latest;
  /// The latest release's notes (Markdown), shown under 设置 → 关于.
  String latestNotes = '';
  AppRelease? available;
  bool checking = false;
  String error = '';
  /// Download progress 0..1 while downloading; null otherwise.
  double? progress;
  /// The finished download of [available], waiting for 安装.
  File? downloaded;
  /// Shown after a desktop install: the App restarts into the new build.
  bool restarting = false;

  static bool get supported => !kIsWeb && (Platform.isAndroid || Platform.isWindows || Platform.isLinux || Platform.isMacOS);

  /// The release file this platform installs.
  static String get _suffix => switch (Platform.operatingSystem) {
        'android' => '-android.apk',
        'linux' => '-linux-x64.tar.gz',
        'macos' => '-macos-universal.dmg',
        _ => '-windows-x64-setup.exe',
      };

  /// `quiet` skips errors (the start-up check); the settings button shows them.
  Future<void> check({bool quiet = false}) async {
    if (checking || !supported) return;
    checking = true;
    error = '';
    notifyListeners();
    try {
      current = current.isEmpty ? (await PackageInfo.fromPlatform()).version : current;
      final response = await http.get(Uri.parse('https://api.github.com/repos/$_repo/releases/latest'), headers: {'accept': 'application/vnd.github+json'}).timeout(const Duration(seconds: 15));
      if (response.statusCode != 200) throw HttpException('GitHub 返回 ${response.statusCode}');
      final release = jsonDecode(response.body) as Map<String, dynamic>;
      final tag = release['tag_name'] as String? ?? '';
      latest = tag.isEmpty ? null : tag;
      latestNotes = release['body'] as String? ?? '';
      final suffix = _suffix;
      final asset = (release['assets'] as List? ?? const []).cast<Map<String, dynamic>>()
          .where((asset) => (asset['name'] as String? ?? '').startsWith('ashira-app-') && (asset['name'] as String).endsWith(suffix))
          .firstOrNull;
      final next = newer(tag, current) && asset != null
          ? AppRelease(tag: tag, page: release['html_url'] as String? ?? '', asset: asset['browser_download_url'] as String, size: (asset['size'] as num?)?.toInt() ?? 0)
          : null;
      // A download stays ready only while it is still the release on offer.
      if (next?.asset != available?.asset) downloaded = null;
      available = next;
    } catch (reason) {
      if (!quiet) error = '检查更新失败：$reason';
    } finally {
      checking = false;
      notifyListeners();
    }
  }

  /// Fetches the release asset; [install] runs it once the user asks.
  Future<void> download() async {
    final release = available;
    if (release == null || progress != null || downloaded != null) return;
    error = '';
    progress = 0;
    notifyListeners();
    try {
      final dir = await _updateDir();
      final file = File('${dir.path}${Platform.pathSeparator}${release.asset.split('/').last}');
      await _download(release.asset, file, release.size);
      downloaded = file;
    } catch (reason) {
      error = '下载失败：$reason';
    } finally {
      progress = null;
      notifyListeners();
    }
  }

  Future<void> install() async {
    final file = downloaded;
    if (file == null || restarting) return;
    error = '';
    notifyListeners();
    try {
      if (!file.existsSync()) {
        downloaded = null;
        throw const _Notice('安装包已被清理，请重新下载');
      }
      if (Platform.isAndroid) {
        // The system page answers when the user comes back; granted, the install goes on.
        if (!(await _channel.invokeMethod<bool>('canInstall') ?? false) && !(await _channel.invokeMethod<bool>('allowInstall') ?? false)) return;
        await _channel.invokeMethod('install', {'path': file.path});
      } else if (Platform.isWindows) {
        await _swapWindows(file, file.parent);
      } else {
        await _swapUnix(file, file.parent);
      }
    } on _Notice catch (notice) {
      error = notice.message;
    } catch (reason) {
      error = '安装失败：$reason';
    } finally {
      notifyListeners();
    }
  }

  static Future<Directory> _updateDir() async {
    final dir = Directory('${(await getTemporaryDirectory()).path}${Platform.pathSeparator}update');
    await dir.create(recursive: true);
    return dir;
  }

  Future<void> _download(String url, File file, int size) async {
    final client = http.Client();
    try {
      final response = await client.send(http.Request('GET', Uri.parse(url))).timeout(const Duration(seconds: 30));
      if (response.statusCode != 200) throw HttpException('下载返回 ${response.statusCode}');
      final total = response.contentLength ?? size;
      final sink = file.openWrite();
      var received = 0;
      await for (final chunk in response.stream) {
        sink.add(chunk);
        received += chunk.length;
        if (total > 0) {
          progress = received / total;
          notifyListeners();
        }
      }
      await sink.close();
    } finally {
      client.close();
    }
  }

  /// Runs the downloaded installer silently once this process has quit, so it can replace
  /// the running files; the installer starts the App again when it is done.
  Future<void> _swapWindows(File setup, Directory dir) async {
    final script = File('${dir.path}\\install.cmd');
    await script.writeAsString([
      '@echo off',
      ':wait',
      'tasklist /FI "PID eq $pid" | find "$pid" >nul && (timeout /t 1 /nobreak >nul & goto wait)',
      // Same folder it is installed in now; no wizard, no prompts, no restart of Windows.
      '"${setup.path}" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /NOCANCEL "/DIR=${File(Platform.resolvedExecutable).parent.path}"',
    ].join('\r\n'));
    await Process.start('cmd.exe', ['/c', script.path], mode: ProcessStartMode.detached);
    restarting = true;
    notifyListeners();
    // Let the UI show 「正在重启」 before the files are replaced.
    await Future<void>.delayed(const Duration(milliseconds: 600));
    exit(0);
  }

  /// Linux and macOS: a script waits for this process to exit, puts the new build where this
  /// one runs from, and starts it. Linux unpacks the release tarball over the install folder;
  /// macOS copies `Ashira.app` out of the release disk image over the running bundle.
  Future<void> _swapUnix(File archive, Directory dir) async {
    final exe = Platform.resolvedExecutable;
    // Linux: the folder holding `ashira_app`. macOS: the `.app` bundle three levels up.
    final target = Platform.isMacOS ? File(exe).parent.parent.parent.path : File(exe).parent.path;
    String q(String path) => "'${path.replaceAll("'", r"'\''")}'";
    final staged = '${dir.path}/unpacked';
    final lines = <String>[
      '#!/bin/sh',
      'set -e',
      'while kill -0 $pid 2>/dev/null; do sleep 1; done',
      'rm -rf ${q(staged)} && mkdir -p ${q(staged)}',
      if (Platform.isLinux) ...[
        'tar xzf ${q(archive.path)} -C ${q(staged)}',
        // The tarball holds one folder (`ashira-app-vX-linux-x64`) with the bundle inside.
        'src=\$(find ${q(staged)} -mindepth 1 -maxdepth 1 -type d | head -n 1)',
        'for item in ashira_app data lib; do rm -rf ${q(target)}/"\$item"; cp -R "\$src/\$item" ${q(target)}/; done',
        'exec ${q(exe)}',
      ] else ...[
        'mount=\$(mktemp -d)',
        'hdiutil attach -nobrowse -readonly -mountpoint "\$mount" ${q(archive.path)} >/dev/null',
        'rm -rf ${q('$target.new')} && cp -R "\$mount/Ashira.app" ${q('$target.new')}',
        'hdiutil detach "\$mount" >/dev/null || true',
        'rm -rf ${q(target)} && mv ${q('$target.new')} ${q(target)}',
        // Downloaded files carry quarantine; the copy should open like the one it replaces.
        'xattr -dr com.apple.quarantine ${q(target)} 2>/dev/null || true',
        'exec open ${q(target)}',
      ],
    ];
    final script = File('${dir.path}/install.sh');
    await script.writeAsString('${lines.join('\n')}\n');
    await Process.start('/bin/sh', [script.path], mode: ProcessStartMode.detached);
    restarting = true;
    notifyListeners();
    await Future<void>.delayed(const Duration(milliseconds: 600));
    exit(0);
  }
}

class _Notice implements Exception {
  const _Notice(this.message);
  final String message;
}

/// `1.2.3` from `v1.2.3`; pre-release suffixes are ignored for ordering.
List<int>? _parse(String version) {
  final core = version.trim().replaceFirst(RegExp('^v'), '').split(RegExp('[-+]')).first;
  final parts = core.split('.').map(int.tryParse).toList();
  if (parts.isEmpty || parts.first == null) return null;
  return [for (var i = 0; i < 3; i++) i < parts.length ? parts[i] ?? 0 : 0];
}

@visibleForTesting
bool newer(String candidate, String current) {
  final a = _parse(candidate), b = _parse(current);
  if (a == null || b == null) return false;
  for (var i = 0; i < 3; i++) {
    if (a[i] != b[i]) return a[i] > b[i];
  }
  return false;
}
