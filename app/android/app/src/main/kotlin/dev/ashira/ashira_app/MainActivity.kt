package dev.ashira.ashira_app

import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.io.File

class MainActivity : FlutterActivity() {
    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        // In-app update: hands a downloaded APK to the system installer.
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "dev.ashira/update").setMethodCallHandler { call, result ->
            when (call.method) {
                "canInstall" -> result.success(Build.VERSION.SDK_INT < Build.VERSION_CODES.O || packageManager.canRequestPackageInstalls())
                "allowInstall" -> {
                    // Android 8+: the user grants "install unknown apps" for Ashira once.
                    startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:$packageName")))
                    result.success(null)
                }
                "install" -> {
                    val path = call.argument<String>("path")
                    if (path == null) {
                        result.error("path", "missing path", null)
                    } else {
                        val uri = FileProvider.getUriForFile(this, "$packageName.update", File(path))
                        startActivity(Intent(Intent.ACTION_VIEW).apply {
                            setDataAndType(uri, "application/vnd.android.package-archive")
                            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
                        })
                        result.success(null)
                    }
                }
                else -> result.notImplemented()
            }
        }
    }
}
