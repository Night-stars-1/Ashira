import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

/// Neutral tokens matching the web client (`web/src/style.css`, shadcn neutral).
class _Tokens {
  const _Tokens.dark()
      : background = const Color(0xFF0A0A0A),
        foreground = const Color(0xFFFAFAFA),
        card = const Color(0xFF171717),
        sidebar = const Color(0xFF171717),
        muted = const Color(0xFF262626),
        mutedForeground = const Color(0xFFA1A1A1),
        primary = const Color(0xFFE5E5E5),
        primaryForeground = const Color(0xFF171717),
        border = const Color(0x1AFFFFFF),
        input = const Color(0x26FFFFFF),
        ring = const Color(0xFF737373),
        destructive = const Color(0xFFFF6467);

  const _Tokens.light()
      : background = const Color(0xFFFFFFFF),
        foreground = const Color(0xFF0A0A0A),
        card = const Color(0xFFFFFFFF),
        sidebar = const Color(0xFFFAFAFA),
        muted = const Color(0xFFF5F5F5),
        mutedForeground = const Color(0xFF737373),
        primary = const Color(0xFF171717),
        primaryForeground = const Color(0xFFFAFAFA),
        border = const Color(0xFFE5E5E5),
        input = const Color(0xFFE5E5E5),
        ring = const Color(0xFFA1A1A1),
        destructive = const Color(0xFFE7000B);

  final Color background, foreground, card, sidebar, muted, mutedForeground, primary, primaryForeground, border, input, ring, destructive;
}

/// Named access to the web tokens mapped onto [ColorScheme] slots.
extension Palette on ColorScheme {
  Color get border => outlineVariant;
  Color get muted => surfaceContainerHigh;
  Color get mutedForeground => onSurfaceVariant;
  Color get sidebar => surfaceContainerLow;
  Color get card => surfaceContainer;
}

bool get isDesktop => {TargetPlatform.windows, TargetPlatform.macOS, TargetPlatform.linux}.contains(defaultTargetPlatform);

/// Control height: compact on desktop, finger-sized on phones.
double get controlHeight => isDesktop ? 28 : 40;

const radius = 8.0;

/// HarmonyOS Sans SC is bundled (see pubspec); the system fonts only fill in missing glyphs.
const _sans = 'HarmonyOS Sans SC';
const _sansFallback = ['Segoe UI', 'Microsoft YaHei UI', 'Microsoft YaHei', 'PingFang SC', 'Noto Sans CJK SC', 'Noto Sans SC'];

ThemeData buildTheme(Brightness brightness) {
  final t = brightness == Brightness.dark ? const _Tokens.dark() : const _Tokens.light();
  final scheme = ColorScheme(
    brightness: brightness,
    primary: t.primary,
    onPrimary: t.primaryForeground,
    primaryContainer: t.muted,
    onPrimaryContainer: t.foreground,
    secondary: t.muted,
    onSecondary: t.foreground,
    secondaryContainer: t.muted,
    onSecondaryContainer: t.foreground,
    tertiary: t.muted,
    onTertiary: t.foreground,
    error: t.destructive,
    onError: Colors.white,
    errorContainer: t.destructive.withValues(alpha: .15),
    onErrorContainer: t.destructive,
    surface: t.background,
    onSurface: t.foreground,
    onSurfaceVariant: t.mutedForeground,
    surfaceContainerLowest: t.background,
    surfaceContainerLow: t.sidebar,
    surfaceContainer: t.card,
    surfaceContainerHigh: t.muted,
    surfaceContainerHighest: t.muted,
    outline: t.ring,
    outlineVariant: t.border,
    surfaceTint: Colors.transparent,
    shadow: Colors.black,
    inverseSurface: t.foreground,
    onInverseSurface: t.background,
  );

  final rounded = RoundedRectangleBorder(borderRadius: BorderRadius.circular(radius));
  final size = Size(0, controlHeight);
  const label = TextStyle(fontFamily: _sans, fontFamilyFallback: _sansFallback, fontSize: 12.8, fontWeight: FontWeight.w500, height: 1.2);
  final text = Typography.material2021(platform: defaultTargetPlatform).englishLike.merge(const TextTheme(
        headlineMedium: TextStyle(fontSize: 26, fontWeight: FontWeight.w600, letterSpacing: -.3),
        titleLarge: TextStyle(fontSize: 18, fontWeight: FontWeight.w600),
        titleMedium: TextStyle(fontSize: 15, fontWeight: FontWeight.w600),
        titleSmall: TextStyle(fontSize: 14, fontWeight: FontWeight.w600),
        // TextField text; height 1.3 keeps a desktop field at shadcn's 32px (h-8).
        bodyLarge: TextStyle(fontSize: 14, height: 1.3),
        bodyMedium: TextStyle(fontSize: 14, height: 1.5),
        bodySmall: TextStyle(fontSize: 12, height: 1.4),
        labelLarge: label,
        labelMedium: TextStyle(fontSize: 12, fontWeight: FontWeight.w500),
        labelSmall: TextStyle(fontSize: 11, fontWeight: FontWeight.w500),
      )).apply(bodyColor: t.foreground, displayColor: t.foreground, fontFamily: _sans, fontFamilyFallback: _sansFallback);

  WidgetStateProperty<Color?> overlay(Color color) => WidgetStateProperty.resolveWith((states) =>
      states.contains(WidgetState.pressed) ? color.withValues(alpha: .9) : states.any({WidgetState.hovered, WidgetState.focused}.contains) ? color.withValues(alpha: .6) : null);

  OutlineInputBorder field(Color color, [double width = 1]) =>
      OutlineInputBorder(borderRadius: BorderRadius.circular(radius), borderSide: BorderSide(color: color, width: width));

  return ThemeData(
    useMaterial3: true,
    brightness: brightness,
    colorScheme: scheme,
    scaffoldBackgroundColor: t.background,
    canvasColor: t.background,
    textTheme: text,
    // Bundled so Windows and Android look the same; the fallbacks cover glyphs it lacks (emoji, rare CJK).
    fontFamily: _sans,
    fontFamilyFallback: _sansFallback,
    visualDensity: VisualDensity.standard,
    materialTapTargetSize: isDesktop ? MaterialTapTargetSize.shrinkWrap : MaterialTapTargetSize.padded,
    splashFactory: isDesktop ? NoSplash.splashFactory : InkRipple.splashFactory,
    hoverColor: t.muted.withValues(alpha: .6),
    highlightColor: Colors.transparent,
    focusColor: t.muted,
    iconTheme: IconThemeData(size: 18, color: t.foreground),
    dividerTheme: DividerThemeData(color: t.border, space: 1, thickness: 1),
    textSelectionTheme: TextSelectionThemeData(cursorColor: t.foreground, selectionColor: t.ring.withValues(alpha: .4), selectionHandleColor: t.foreground),
    appBarTheme: AppBarTheme(
      backgroundColor: t.background,
      foregroundColor: t.foreground,
      surfaceTintColor: Colors.transparent,
      elevation: 0,
      scrolledUnderElevation: 0,
      toolbarHeight: 48,
      titleSpacing: 12,
      titleTextStyle: text.titleSmall!.copyWith(fontWeight: FontWeight.w500),
      iconTheme: IconThemeData(size: 18, color: t.foreground),
      actionsIconTheme: IconThemeData(size: 18, color: t.foreground),
      shape: Border(bottom: BorderSide(color: t.border)),
    ),
    iconButtonTheme: IconButtonThemeData(
      style: ButtonStyle(
        iconSize: const WidgetStatePropertyAll(16),
        minimumSize: WidgetStatePropertyAll(Size.square(isDesktop ? 32 : 40)),
        fixedSize: WidgetStatePropertyAll(Size.square(isDesktop ? 32 : 40)),
        padding: const WidgetStatePropertyAll(EdgeInsets.zero),
        shape: WidgetStatePropertyAll(rounded),
        foregroundColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.disabled) ? t.mutedForeground.withValues(alpha: .5) : t.foreground),
        overlayColor: overlay(t.muted),
        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
      ),
    ),
    filledButtonTheme: FilledButtonThemeData(
      style: ButtonStyle(
        minimumSize: WidgetStatePropertyAll(size),
        padding: const WidgetStatePropertyAll(EdgeInsets.symmetric(horizontal: 12)),
        shape: WidgetStatePropertyAll(rounded),
        textStyle: const WidgetStatePropertyAll(label),
        iconSize: const WidgetStatePropertyAll(16),
        elevation: const WidgetStatePropertyAll(0),
        backgroundColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.disabled) ? t.primary.withValues(alpha: .4) : t.primary),
        foregroundColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.disabled) ? t.primaryForeground.withValues(alpha: .7) : t.primaryForeground),
        overlayColor: WidgetStatePropertyAll(t.primaryForeground.withValues(alpha: .08)),
        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
      ),
    ),
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: ButtonStyle(
        minimumSize: WidgetStatePropertyAll(size),
        padding: const WidgetStatePropertyAll(EdgeInsets.symmetric(horizontal: 12)),
        shape: WidgetStatePropertyAll(rounded),
        textStyle: const WidgetStatePropertyAll(label),
        iconSize: const WidgetStatePropertyAll(16),
        side: WidgetStatePropertyAll(BorderSide(color: t.input)),
        // `input` is already translucent; scale its alpha like CSS `bg-input/30` instead of replacing it.
        backgroundColor: WidgetStatePropertyAll(brightness == Brightness.dark ? t.input.withValues(alpha: t.input.a * .3) : t.background),
        foregroundColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.disabled) ? t.mutedForeground : t.foreground),
        overlayColor: overlay(t.muted),
        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
      ),
    ),
    textButtonTheme: TextButtonThemeData(
      style: ButtonStyle(
        minimumSize: WidgetStatePropertyAll(size),
        padding: const WidgetStatePropertyAll(EdgeInsets.symmetric(horizontal: 10)),
        shape: WidgetStatePropertyAll(rounded),
        textStyle: const WidgetStatePropertyAll(label),
        iconSize: const WidgetStatePropertyAll(16),
        foregroundColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.disabled) ? t.mutedForeground : t.foreground),
        overlayColor: overlay(t.muted),
        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
      ),
    ),
    inputDecorationTheme: InputDecorationTheme(
      isDense: true,
      filled: brightness == Brightness.dark,
      fillColor: t.input.withValues(alpha: t.input.a * .3),
      // 14px × 1.3 text + 2 × 6px padding + 2px border ≈ 32px, the web's input height.
      contentPadding: EdgeInsets.symmetric(horizontal: 10, vertical: isDesktop ? 6 : 10),
      // Same line height as the input text, or the hint line sets a taller field.
      hintStyle: TextStyle(color: t.mutedForeground, fontSize: 14, height: 1.3),
      labelStyle: TextStyle(color: t.mutedForeground, fontSize: 14, height: 1.3),
      floatingLabelBehavior: FloatingLabelBehavior.never,
      helperStyle: TextStyle(color: t.mutedForeground, fontSize: 12),
      prefixIconColor: t.mutedForeground,
      suffixIconColor: t.mutedForeground,
      prefixIconConstraints: const BoxConstraints(minWidth: 32, minHeight: 0),
      suffixIconConstraints: const BoxConstraints(minWidth: 32, minHeight: 0),
      border: field(t.input),
      enabledBorder: field(t.input),
      disabledBorder: field(t.input.withValues(alpha: t.input.a * .5)),
      focusedBorder: field(t.ring),
      errorBorder: field(t.destructive),
      focusedErrorBorder: field(t.destructive),
    ),
    dialogTheme: DialogThemeData(
      barrierColor: Colors.black.withValues(alpha: .6),
      backgroundColor: t.sidebar,
      surfaceTintColor: Colors.transparent,
      elevation: 8,
      shadowColor: Colors.black.withValues(alpha: .4),
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(14), side: BorderSide(color: t.border)),
      titleTextStyle: text.titleMedium,
      contentTextStyle: text.bodyMedium!.copyWith(color: t.mutedForeground),
      insetPadding: const EdgeInsets.all(16),
      actionsPadding: const EdgeInsets.fromLTRB(20, 0, 20, 20),
    ),
    drawerTheme: DrawerThemeData(backgroundColor: t.sidebar, surfaceTintColor: Colors.transparent, shape: const RoundedRectangleBorder(), width: 300),
    listTileTheme: ListTileThemeData(
      dense: true,
      contentPadding: const EdgeInsets.symmetric(horizontal: 16),
      horizontalTitleGap: 10,
      minLeadingWidth: 18,
      minVerticalPadding: 8,
      iconColor: t.mutedForeground,
      textColor: t.foreground,
      titleTextStyle: text.bodyMedium,
      subtitleTextStyle: text.bodySmall!.copyWith(color: t.mutedForeground),
      selectedColor: t.foreground,
      selectedTileColor: t.muted,
    ),
    chipTheme: ChipThemeData(
      showCheckmark: false,
      backgroundColor: Colors.transparent,
      selectedColor: t.muted,
      side: BorderSide(color: t.border),
      shape: rounded,
      labelStyle: TextStyle(fontSize: 12, fontWeight: FontWeight.w500, color: t.foreground),
      padding: const EdgeInsets.symmetric(horizontal: 4),
    ),
    segmentedButtonTheme: SegmentedButtonThemeData(
      style: ButtonStyle(
        visualDensity: VisualDensity.compact,
        textStyle: const WidgetStatePropertyAll(label),
        side: WidgetStatePropertyAll(BorderSide(color: t.input)),
        shape: WidgetStatePropertyAll(rounded),
        backgroundColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.selected) ? t.muted : Colors.transparent),
        foregroundColor: WidgetStatePropertyAll(t.foreground),
      ),
      selectedIcon: const SizedBox.shrink(),
    ),
    tabBarTheme: TabBarThemeData(
      labelColor: t.foreground,
      unselectedLabelColor: t.mutedForeground,
      labelStyle: label,
      unselectedLabelStyle: label,
      indicatorColor: t.foreground,
      indicatorSize: TabBarIndicatorSize.label,
      dividerColor: t.border,
      overlayColor: WidgetStatePropertyAll(t.muted.withValues(alpha: .5)),
    ),
    switchTheme: SwitchThemeData(
      trackColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.selected) ? t.primary : t.input),
      thumbColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.selected) ? t.primaryForeground : t.foreground),
      trackOutlineColor: const WidgetStatePropertyAll(Colors.transparent),
    ),
    tooltipTheme: TooltipThemeData(
      decoration: BoxDecoration(color: t.foreground, borderRadius: BorderRadius.circular(6)),
      textStyle: TextStyle(color: t.background, fontSize: 12),
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
      waitDuration: const Duration(milliseconds: 500),
    ),
    scrollbarTheme: ScrollbarThemeData(
      thickness: const WidgetStatePropertyAll(6),
      radius: const Radius.circular(3),
      thumbColor: WidgetStatePropertyAll(t.foreground.withValues(alpha: .18)),
    ),
    snackBarTheme: SnackBarThemeData(
      behavior: SnackBarBehavior.floating,
      backgroundColor: t.foreground,
      contentTextStyle: TextStyle(color: t.background, fontSize: 13),
      shape: rounded,
      width: isDesktop ? 360 : null,
    ),
    progressIndicatorTheme: ProgressIndicatorThemeData(color: t.mutedForeground, strokeWidth: 2),
    cardTheme: CardThemeData(
      color: t.card,
      surfaceTintColor: Colors.transparent,
      elevation: 0,
      margin: EdgeInsets.zero,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10), side: BorderSide(color: t.border)),
    ),
    dataTableTheme: DataTableThemeData(
      headingTextStyle: TextStyle(fontSize: 12, fontWeight: FontWeight.w500, color: t.mutedForeground),
      dataTextStyle: TextStyle(fontSize: 12, color: t.foreground, fontFeatures: const [FontFeature.tabularFigures()]),
      dividerThickness: 1,
    ),
    popupMenuTheme: PopupMenuThemeData(
      color: t.card,
      surfaceTintColor: Colors.transparent,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(radius), side: BorderSide(color: t.border)),
      textStyle: text.bodyMedium,
    ),
    menuTheme: MenuThemeData(
      style: MenuStyle(
        backgroundColor: WidgetStatePropertyAll(t.card),
        surfaceTintColor: const WidgetStatePropertyAll(Colors.transparent),
        padding: const WidgetStatePropertyAll(EdgeInsets.all(4)),
        shape: WidgetStatePropertyAll(RoundedRectangleBorder(borderRadius: BorderRadius.circular(radius), side: BorderSide(color: t.border))),
      ),
    ),
    menuButtonTheme: MenuButtonThemeData(
      style: ButtonStyle(
        minimumSize: WidgetStatePropertyAll(Size(0, isDesktop ? 30 : 40)),
        padding: const WidgetStatePropertyAll(EdgeInsets.symmetric(horizontal: 8)),
        shape: WidgetStatePropertyAll(RoundedRectangleBorder(borderRadius: BorderRadius.circular(6))),
        textStyle: WidgetStatePropertyAll(text.bodyMedium?.copyWith(fontSize: 13)),
        overlayColor: WidgetStatePropertyAll(t.muted),
      ),
    ),
  );
}

TextStyle monoStyle(BuildContext context) => TextStyle(
      fontFamily: 'Cascadia Mono',
      fontFamilyFallback: const ['Consolas', 'Menlo', 'Roboto Mono', 'monospace', 'Microsoft YaHei', 'Noto Sans Mono CJK SC'],
      color: Theme.of(context).colorScheme.onSurface,
    );

Color linkColor(BuildContext context) =>
    Theme.of(context).brightness == Brightness.dark ? const Color(0xFF38BDF8) : const Color(0xFF0284C7);

const success = Color(0xFF10B981);
const warning = Color(0xFFF59E0B);
