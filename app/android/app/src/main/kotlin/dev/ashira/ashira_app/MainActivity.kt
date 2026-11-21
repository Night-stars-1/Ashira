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
    /// The `allowInstall` call waiting for the user to come back from the permission page.
    private var pendingAllow: MethodChannel.Result? = null

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        // In-app update: hands a downloaded APK to the system installer.
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "dev.ashira/update").setMethodCallHandler { call, result ->
            when (call.method) {
                "canInstall" -> result.success(canInstall())
                "allowInstall" -> {
                    // Android 8+: the user grants "install unknown apps" for Ashira once; the
                    // call answers whether they did when they return to the App.
                    pendingAllow?.success(canInstall())
                    pendingAllow = result
                    @Suppress("DEPRECATION")
                    startActivityForResult(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:$packageName")), ALLOW_INSTALL)
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

    private fun canInstall() = Build.VERSION.SDK_INT < Build.VERSION_CODES.O || packageManager.canRequestPackageInstalls()

    @Deprecated("Deprecated in Java")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
        // The settings page returns no result code; whether the switch is on is the answer.
        if (requestCode == ALLOW_INSTALL) {
            pendingAllow?.success(canInstall())
            pendingAllow = null
        }
    }

    private companion object {
        const val ALLOW_INSTALL = 4021
    }
}
