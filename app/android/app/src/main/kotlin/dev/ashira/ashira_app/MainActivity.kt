package dev.ashira.ashira_app

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
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
    /// Replies: the live update and alerts (see [ReplyNotifications]); tells Dart which
    /// session a tapped notification is about.
    private var replies: MethodChannel? = null

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        replies = MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "dev.ashira/replies").apply {
            setMethodCallHandler { call, result ->
                when (call.method) {
                    "update" -> {
                        ReplyNotifications.update(this@MainActivity, call.argument<List<Map<String, Any?>>>("sessions") ?: emptyList())
                        result.success(null)
                    }
                    "alert" -> {
                        ReplyNotifications.alert(this@MainActivity, call.argument("id")!!, call.argument("title")!!, call.argument("text")!!, call.argument("detail") ?: "",
                            done = call.argument<Boolean>("done") ?: true, chip = call.argument("chip") ?: "已完成")
                        result.success(null)
                    }
                    "cancel" -> {
                        ReplyNotifications.cancel(this@MainActivity, call.argument("id")!!)
                        result.success(null)
                    }
                    // Android 13+ asks once; later the switch is in the App's settings.
                    "permission" -> {
                        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
                            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), NOTIFICATIONS)
                        }
                        result.success(null)
                    }
                    // The session the notification that opened the App is about, once.
                    "launchSession" -> {
                        result.success(intent?.getStringExtra(ReplyNotifications.SESSION))
                        intent?.removeExtra(ReplyNotifications.SESSION)
                    }
                    else -> result.notImplemented()
                }
            }
        }
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

    /// A notification tapped while the App is open (`singleTop`): show that session.
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        intent.getStringExtra(ReplyNotifications.SESSION)?.let { replies?.invokeMethod("open", it) }
    }

    /// HyperOS leaves out of its island what changes while the App is in front, and does
    /// not look again when it goes: the live notification is posted anew then.
    override fun onStop() {
        super.onStop()
        ReplyNotifications.repost(this)
    }

    override fun onDestroy() {
        // The App is gone (swiped away): nothing would end the live update any more.
        if (isFinishing) ReplyNotifications.stop(this)
        super.onDestroy()
    }

    private companion object {
        const val ALLOW_INSTALL = 4021
        const val NOTIFICATIONS = 4022
    }
}
