import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:archive/archive_io.dart';
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
/// it only when asked. Android hands the APK to the system installer; Windows unpacks the
/// zip and swaps it in with a small script once the App quits.
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
  /// Shown after a Windows install: the App restarts into the new build.
  bool restarting = false;

  static bool get supported => !kIsWeb && (Platform.isAndroid || Platform.isWindows);

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
      final suffix = Platform.isAndroid ? '-android.apk' : '-windows-x64.zip';
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
      } else {
        await _swapWindows(file, file.parent);
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

  /// Unpacks the release zip and runs a script that waits for this process to exit,
  /// copies the new build over the install folder and starts it again.
  Future<void> _swapWindows(File zip, Directory dir) async {
    final unpacked = Directory('${dir.path}\\unpacked');
    if (unpacked.existsSync()) unpacked.deleteSync(recursive: true);
    await extractFileToDisk(zip.path, unpacked.path);
    // The zip holds one folder (`ashira-app-vX-windows-x64`) with the build inside.
    final root = unpacked.listSync().whereType<Directory>().firstOrNull ?? unpacked;
    final exe = Platform.resolvedExecutable;
    final install = File(exe).parent.path;
    final script = File('${dir.path}\\swap.cmd');
    await script.writeAsString([
      '@echo off',
      ':wait',
      'tasklist /FI "PID eq $pid" | find "$pid" >nul && (timeout /t 1 /nobreak >nul & goto wait)',
      'robocopy "${root.path}" "$install" /E /NFL /NDL /NJH /NJS /NP >nul',
      'start "" "$exe"',
    ].join('\r\n'));
    await Process.start('cmd.exe', ['/c', script.path], mode: ProcessStartMode.detached);
    restarting = true;
    notifyListeners();
    // Let the UI show 「正在重启」 before the files are replaced.
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
