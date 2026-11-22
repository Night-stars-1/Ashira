import 'package:flutter/material.dart';
import 'package:url_launcher/url_launcher.dart';

import '../api/models.dart';
import '../state/app_update.dart';
import '../state/ashira_state.dart';
import 'markdown.dart';
import 'theme.dart';
import 'widgets.dart';

const _releases = 'https://github.com/Night-stars-1/Ashira/releases';

String _shown(String version) => version.isEmpty ? '未知' : 'v${version.replaceFirst(RegExp('^v'), '')}';

/// Settings → 关于: the App and server versions, and the App's in-place update.
class AboutSettings extends StatelessWidget {
  const AboutSettings({super.key, required this.state});
  final AshiraState state;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final muted = TextStyle(fontSize: 12, color: scheme.mutedForeground);
    return ListenableBuilder(
      // The App updater and the server's version / restart state.
      listenable: Listenable.merge([appUpdate, state]),
      builder: (context, _) {
        final release = appUpdate.available;
        final busy = appUpdate.checking || appUpdate.progress != null || appUpdate.restarting;
        final ready = appUpdate.downloaded != null;
        final serverBehind = state.serverBehind;
        return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
          Row(children: [
            ClipRRect(borderRadius: BorderRadius.circular(12), child: Image.asset('assets/images/app_icon.png', width: 48, height: 48)),
            const SizedBox(width: 12),
            Expanded(
              child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                const Text('Ashira', style: TextStyle(fontSize: 16, fontWeight: FontWeight.w500)),
                const SizedBox(height: 2),
                Text('最新发布 ${appUpdate.checking ? '检查中…' : appUpdate.latest ?? '未知'}', style: muted),
              ]),
            ),
            if (AppUpdate.supported)
              OutlinedButton.icon(
                onPressed: busy ? null : appUpdate.check,
                icon: appUpdate.checking
                    ? const SizedBox.square(dimension: 14, child: CircularProgressIndicator(strokeWidth: 2))
                    : const Icon(Icons.refresh, size: 16),
                label: Text(appUpdate.checking ? '检查中' : '检查更新'),
              ),
          ]),
          if (appUpdate.error.isNotEmpty) ...[
            const SizedBox(height: 12),
            Text(appUpdate.error, style: TextStyle(fontSize: 13, color: scheme.error)),
          ],
          const SizedBox(height: 16),
          Bordered(children: [
            _Row(
              title: 'App',
              version: appUpdate.current.isEmpty ? '未知' : _shown(appUpdate.current),
              trailing: [
                if (release != null) StatusTag('可更新到 ${release.tag}', accent: true)
                else if (appUpdate.latest != null) const StatusTag('已是最新'),
                if (release != null) ...[
                  const SizedBox(width: 8),
                  FilledButton(onPressed: busy ? null : (ready ? appUpdate.install : appUpdate.download), child: Text(_actionLabel())),
                ],
              ],
              below: appUpdate.progress != null ? _ProgressBar(share: appUpdate.progress! > 0 ? appUpdate.progress : null) : null,
            ),
            _Row(
              title: '服务端',
              version: _shown(state.serverVersion),
              trailing: [
                if (state.restartPending) ...[
                  const StatusTag('已更新，待重启', accent: true),
                  const SizedBox(width: 8),
                  FilledButton(onPressed: state.restarting ? null : state.restartServer, child: Text(state.restarting ? '重启中…' : '重启')),
                ] else if (serverBehind != null && state.serverInContainer)
                  // A container is updated by pulling its image; nothing to press here.
                  Tooltip(message: 'docker compose pull && docker compose up -d', child: StatusTag('可更新到 $serverBehind，请拉取新镜像', accent: true))
                else if (serverBehind != null) ...[
                  StatusTag('可更新到 $serverBehind', accent: true),
                  const SizedBox(width: 8),
                  FilledButton(onPressed: state.updating ? null : state.updateServer, child: Text(state.updating ? '更新中…' : '更新')),
                ] else if (state.serverVersion.isNotEmpty)
                  const StatusTag('已是最新'),
              ],
              below: state.updating ? _ProgressBar(share: _share(state.updateProgress)) : null,
            ),
          ]),
          if (appUpdate.latestNotes.isNotEmpty) ...[
            const SizedBox(height: 16),
            _ReleaseNotes(tag: appUpdate.latest ?? '', notes: appUpdate.latestNotes),
          ],
          const SizedBox(height: 12),
          Align(
            alignment: Alignment.centerLeft,
            child: TextButton.icon(
              onPressed: () => launchUrl(Uri.parse(_releases), mode: LaunchMode.externalApplication),
              icon: const Icon(Icons.open_in_new, size: 14),
              label: const Text('所有版本与更新说明', style: TextStyle(fontSize: 12)),
              style: TextButton.styleFrom(foregroundColor: scheme.mutedForeground, padding: const EdgeInsets.symmetric(horizontal: 4)),
            ),
          ),
        ]);
      },
    );
  }

  /// 下载 → 下载中 → 安装 → 正在重启: installing waits for the user's tap; the bar below shows how far.
  static String _actionLabel() {
    if (appUpdate.restarting) return '正在重启…';
    if (appUpdate.progress != null) return '下载中…';
    return appUpdate.downloaded != null ? '安装' : '下载';
  }

  /// The server download's share, or null while its size is unknown.
  static double? _share(UpdateProgress? download) {
    final total = download?.total;
    return download != null && total != null ? (download.received / total).clamp(0.0, 1.0) : null;
  }
}

class _Row extends StatelessWidget {
  const _Row({required this.title, required this.version, required this.trailing, this.below});
  final String title, version;
  final List<Widget> trailing;
  /// Shown under the row, e.g. the server update's progress.
  final Widget? below;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 12),
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        Row(children: [
          Expanded(
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Text(title, style: const TextStyle(fontSize: 14)),
              const SizedBox(height: 2),
              Text(version, style: TextStyle(fontSize: 12, color: scheme.mutedForeground, fontFeatures: const [FontFeature.tabularFigures()])),
            ]),
          ),
          ...trailing,
        ]),
        if (below != null) ...[const SizedBox(height: 8), below!],
      ]),
    );
  }
}

/// A download under its row, its percentage on the right; `share` null spins indeterminately.
class _ProgressBar extends StatelessWidget {
  const _ProgressBar({required this.share});
  final double? share;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final share = this.share;
    return Row(children: [
      Expanded(
        child: ClipRRect(
          borderRadius: BorderRadius.circular(3),
          child: LinearProgressIndicator(value: share, minHeight: 6, backgroundColor: scheme.surfaceContainerHighest, color: scheme.primary),
        ),
      ),
      SizedBox(
        width: 44,
        child: Text(
          share == null ? '' : '${(share * 100).round()}%',
          textAlign: TextAlign.right,
          style: TextStyle(fontSize: 12, color: scheme.mutedForeground, fontFeatures: const [FontFeature.tabularFigures()]),
        ),
      ),
    ]);
  }
}

/// The release notes on a gray card, folded until the header row is tapped.
class _ReleaseNotes extends StatefulWidget {
  const _ReleaseNotes({required this.tag, required this.notes});
  final String tag, notes;

  @override
  State<_ReleaseNotes> createState() => _ReleaseNotesState();
}

class _ReleaseNotesState extends State<_ReleaseNotes> {
  bool _open = false;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Material(
      color: scheme.muted,
      borderRadius: BorderRadius.circular(radius),
      clipBehavior: Clip.antiAlias,
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        InkWell(
          onTap: () => setState(() => _open = !_open),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
            child: Row(children: [
              Expanded(child: Text('${widget.tag} 更新说明', style: const TextStyle(fontSize: 14))),
              AnimatedRotation(
                turns: _open ? .5 : 0,
                duration: const Duration(milliseconds: 300),
                child: Icon(Icons.keyboard_arrow_down, size: 18, color: scheme.mutedForeground),
              ),
            ]),
          ),
        ),
        // Grows from the header and shrinks back; ease-in-out so the first frames do not
        // already show half the notes (easeOut read as a pop).
        AnimatedSize(
          duration: const Duration(milliseconds: 300),
          curve: Curves.easeInOutCubic,
          alignment: Alignment.topCenter,
          child: _open ? Padding(padding: const EdgeInsets.fromLTRB(16, 0, 16, 12), child: Markdown(widget.notes)) : const SizedBox(width: double.infinity),
        ),
      ]),
    );
  }
}
