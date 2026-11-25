package dev.ashira.ashira_app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.drawable.Icon
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.SystemClock
import org.json.JSONArray
import org.json.JSONObject

/// While replies run: one ongoing notification Android 16+ promotes to a Live Update (the
/// status bar chip, on Xiaomi HyperOS its Super Island, and a pinned card), shown by
/// [ReplyService] so the App keeps its connection to the server in the background.
///
/// A reply that asks the user something or ends is told on the island while it shows (the
/// question as 「待回答」, an end as 「已完成」 for a few seconds, then a silent entry in the
/// shade), else by a pop-up alert; never both. Tapping any of them opens that session.
/// Android 8+ (channels). Main thread only.
object ReplyNotifications {
    /// The intent extra naming the session a tapped notification is about.
    const val SESSION = "dev.ashira.session"
    const val LIVE_ID = 1
    private const val ALERT_ID = 2
    private const val LIVE_CHANNEL = "replies"
    private const val ALERT_CHANNEL = "reply_alerts"
    private const val RESULT_CHANNEL = "reply_results"
    /// How long the island shows that a reply ended.
    private const val ENDED_MS = 8_000L
    /// The App icon in the island template's `miui.focus.pics`.
    private const val ISLAND_PIC = "miui.focus.pic_app"
    /// The 「去回答」 button in the template's `miui.focus.actions`.
    private const val ISLAND_ANSWER = "miui.focus.action_answer"

    /// A reply that just ended, on the island until `until` (uptime ms). `chip`: its short text.
    private class Ended(val id: String, val title: String, val text: String, val detail: String, val chip: String, val until: Long)

    /// The ongoing notification [ReplyService] shows; kept after the last reply ends, so a
    /// service start racing [end] still has one to start with.
    @Volatile var live: Notification? = null
        private set
    @Volatile var active = false
        private set
    private var serviceStarted = false
    /// The running sessions, oldest first, each `id`, `title`, `asking` and `since` (epoch ms).
    private var sessions: List<Map<String, Any?>> = emptyList()
    private val ended = mutableListOf<Ended>()
    private val handler = Handler(Looper.getMainLooper())

    /// Empty `sessions` ends the live update once no ended reply is left on it.
    fun update(context: Context, sessions: List<Map<String, Any?>>) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        this.sessions = sessions
        refresh(context.applicationContext)
    }

    /// Posts the live notification again as it is: on HyperOS, for its island to take what
    /// changed while the App was in front.
    fun repost(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O || !active || !islandOs(context)) return
        refresh(context.applicationContext)
    }

    /// Ends it all now (the App is closing): ended replies still on the island go to the shade.
    fun stop(context: Context) {
        handler.removeCallbacksAndMessages(null)
        ended.forEach { notify(context, it.id, RESULT_CHANNEL, it.title, it.text, it.detail) }
        ended.clear()
        sessions = emptyList()
        end(context)
    }

    /// `done`: the reply ended (`chip` says how); else it asks the user something. One alert
    /// per session: a later one (finished after asking) replaces the earlier.
    fun alert(context: Context, id: String, title: String, text: String, detail: String, done: Boolean, chip: String) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        channels(context)
        if (islandShown(context)) {
            // A question shows on the island as 「待回答」 with the update that follows.
            if (done) {
                ended.removeAll { it.id == id }
                ended += Ended(id, title, text, detail, chip, SystemClock.uptimeMillis() + ENDED_MS)
                refresh(context.applicationContext)
            }
            return
        }
        notify(context, id, ALERT_CHANNEL, title, text, detail)
    }

    /// The user opened this session: its alert, and its end on the island, have been seen.
    fun cancel(context: Context, id: String) {
        context.getSystemService(NotificationManager::class.java).cancel(id, ALERT_ID)
        if (ended.removeAll { it.id == id }) refresh(context.applicationContext)
    }

    private fun refresh(context: Context) {
        handler.removeCallbacksAndMessages(null)
        val now = SystemClock.uptimeMillis()
        for (gone in ended.filter { it.until <= now }) {
            ended.remove(gone)
            notify(context, gone.id, RESULT_CHANNEL, gone.title, gone.text, gone.detail)
        }
        if (sessions.isEmpty() && ended.isEmpty()) {
            end(context)
            return
        }
        channels(context)
        show(context, build(context, ended.lastOrNull()))
        ended.minOfOrNull { it.until }?.let { handler.postAtTime({ refresh(context) }, it) }
    }

    private fun show(context: Context, notification: Notification) {
        live = notification
        active = true
        val manager = context.getSystemService(NotificationManager::class.java)
        if (serviceStarted) {
            manager.notify(LIVE_ID, notification)
            return
        }
        try {
            context.startForegroundService(Intent(context, ReplyService::class.java))
            serviceStarted = true
        } catch (_: IllegalStateException) {
            // Android 12+ starts no foreground service from the background (a reply started
            // on the web while the App was away). Without one the App is frozen within
            // seconds and could never end an ongoing notification: post none, and leave it
            // to the alerts; the next update from the foreground starts the service.
            manager.cancel(LIVE_ID)
        }
    }

    private fun end(context: Context) {
        active = false
        // The service ends itself once it has gone to the foreground: stopping one started
        // with startForegroundService before it did would crash the App. Its foreground
        // state lets the App start it from the background.
        if (serviceStarted) {
            try {
                context.startService(Intent(context, ReplyService::class.java))
            } catch (_: IllegalStateException) {
                context.stopService(Intent(context, ReplyService::class.java))
            }
        }
        serviceStarted = false
        context.getSystemService(NotificationManager::class.java).cancel(LIVE_ID)
    }

    /// Whether the island shows the replies right now: the system chip (the live update was
    /// promoted), or on HyperOS its island window.
    private fun islandShown(context: Context): Boolean =
        context.getSystemService(NotificationManager::class.java).activeNotifications.any {
            it.id == LIVE_ID && it.tag == null && (
                (Build.VERSION.SDK_INT >= Build.VERSION_CODES.BAKLAVA && (it.notification.flags and Notification.FLAG_PROMOTED_ONGOING) != 0) ||
                    it.notification.extras.containsKey("miui.focus.param"))
        }

    /// Xiaomi HyperOS 3 and later: the system shows Super Islands from the template
    /// (`notification_focus_protocol` 3, per the developer guide's query section).
    private fun islandOs(context: Context): Boolean =
        android.provider.Settings.System.getInt(context.contentResolver, "notification_focus_protocol", 0) >= 3

    /// A pop-up alert, or with [RESULT_CHANNEL] a silent entry in the shade.
    private fun notify(context: Context, id: String, channel: String, title: String, text: String, detail: String) {
        val notification = Notification.Builder(context, channel)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(title)
            .setContentText(text)
            .setStyle(Notification.BigTextStyle().bigText(if (detail.isEmpty()) text else "$text\n$detail"))
            .setCategory(Notification.CATEGORY_STATUS)
            .setAutoCancel(true)
            .setContentIntent(open(context, id))
            .build()
        context.getSystemService(NotificationManager::class.java).notify(id, ALERT_ID, notification)
    }

    /// The running replies, or `finished` (the latest that just ended) while it shows.
    private fun build(context: Context, finished: Ended?): Notification {
        val hyperOs = islandOs(context)
        val builder = Notification.Builder(context, LIVE_CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setCategory(Notification.CATEGORY_PROGRESS)
        // Android 16: ask for the promotion to a Live Update (`setRequestPromotedOngoing`),
        // shown as the status bar chip. Not on HyperOS: it draws the island from the template
        // in its own window, and not for a notification the chip already shows.
        if (!hyperOs) builder.addExtras(Bundle().apply { putBoolean("android.requestPromotedOngoing", true) })
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) builder.setForegroundServiceBehavior(Notification.FOREGROUND_SERVICE_IMMEDIATE)
        if (finished != null) {
            builder.setContentTitle(finished.title)
                .setContentText(finished.text)
                .setContentIntent(open(context, finished.id))
            if (finished.detail.isNotEmpty()) builder.setStyle(Notification.BigTextStyle().bigText("${finished.text}\n${finished.detail}"))
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.BAKLAVA) builder.setShortCriticalText(finished.chip)
            if (hyperOs) builder.addExtras(island(context, finished.id, finished.title, finished.detail.ifEmpty { finished.text }, finished.chip, null, expand = true, answer = false))
            return builder.build()
        }
        val asking = sessions.filter { it["asking"] == true }
        val first = asking.firstOrNull() ?: sessions.first()
        val single = sessions.size == 1
        val title = if (single) first["title"] as? String ?: "Ashira" else "${sessions.size} 个会话正在回复"
        val text = when {
            single -> if (asking.isEmpty()) "正在回复" else "等待你回答"
            asking.isEmpty() -> sessions.joinToString("、") { it["title"] as? String ?: "" }
            else -> "${asking.size} 个在等你回答"
        }
        val since = sessions.minOf { (it["since"] as? Number)?.toLong() ?: System.currentTimeMillis() }
        builder.setContentTitle(title)
            .setContentText(text)
            // The elapsed time on the card and in the chip.
            .setWhen(since)
            .setShowWhen(true)
            .setUsesChronometer(true)
            .setContentIntent(open(context, first["id"] as String))
        // The chip's text, in place of the elapsed time, while an answer is wanted.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.BAKLAVA && asking.isNotEmpty()) builder.setShortCriticalText("待回答")
        if (hyperOs) {
            // The card's body: the question when one waits (only one session can be shown),
            // else what the replies are doing.
            val question = (first["question"] as? String).orEmpty()
            val body = if (single && question.isNotEmpty()) question else text
            builder.addExtras(island(context, first["id"] as String, title, body, if (asking.isEmpty()) "回复中" else "待回答", since, expand = asking.isNotEmpty(), answer = asking.isNotEmpty()))
        }
        return builder.build()
    }

    /// Xiaomi HyperOS Super Island template (`miui.focus.param`, per the developer guide,
    /// dev.mi.com/xiaomihyperos/documentation/detail?pId=2131). The island: the App icon with
    /// `state`, and a timer from `since` when given. Opened up, a chat card: the App icon,
    /// `title` (the session), `body` (what it does, its question, or its answer), the same
    /// timer, and with `answer` a 「去回答」 button. `expand` opens it up when it shows.
    private fun island(context: Context, id: String, title: String, body: String, state: String, since: Long?, expand: Boolean, answer: Boolean): Bundle {
        val icon = JSONObject().put("type", 1).put("pic", ISLAND_PIC)
        // Counts up from `since`, kept by the system without updates.
        val timer = since?.let { JSONObject().put("timerType", 1).put("timerWhen", it).put("timerSystemCurrent", System.currentTimeMillis()) }
        val big = JSONObject().put("imageTextInfoLeft", JSONObject().put("type", 1).put("picInfo", icon).put("textInfo", JSONObject().put("title", state)))
        if (timer != null) big.put("sameWidthDigitInfo", JSONObject().put("timerInfo", timer))
        val chat = JSONObject().put("picProfile", ISLAND_PIC).put("title", title).put("content", body)
        if (timer != null) chat.put("timerInfo", timer)
        val params = JSONObject()
            .put("protocol", 1)
            .put("business", "ai_reply")
            .put("updatable", true)
            // The live notification comes back with each reply after the last one ended; the
            // default `close` keeps an island once cancelled from showing again.
            .put("reopen", "reopen")
            .put("islandFirstFloat", expand)
            .put("enableFloat", expand)
            .put("ticker", state)
            .put("aodTitle", "$title $state")
            .put("chatInfo", chat)
            .put("param_island", JSONObject()
                .put("islandProperty", if (answer) 2 else 1)
                .put("bigIslandArea", big)
                .put("smallIslandArea", JSONObject().put("picInfo", icon)))
        // A text button: the plain `actions` one shows only its icon.
        if (answer) params.put("textButton", JSONArray().put(JSONObject().put("action", ISLAND_ANSWER).put("actionTitle", "去回答")))
        return Bundle().apply {
            putString("miui.focus.param", JSONObject().put("param_v2", params).toString())
            putBundle("miui.focus.pics", Bundle().apply { putParcelable(ISLAND_PIC, Icon.createWithResource(context, R.mipmap.ic_launcher)) })
            if (answer) putBundle("miui.focus.actions", Bundle().apply {
                putParcelable(ISLAND_ANSWER, Notification.Action.Builder(Icon.createWithResource(context, R.drawable.ic_notification), "去回答", open(context, id)).build())
            })
        }
    }

    private fun open(context: Context, id: String): PendingIntent {
        val intent = Intent(context, MainActivity::class.java).putExtra(SESSION, id).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP)
        // Extras do not tell pending intents apart: one request code per session does.
        return PendingIntent.getActivity(context, id.hashCode(), intent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
    }

    private fun channels(context: Context) {
        val manager = context.getSystemService(NotificationManager::class.java)
        // Low: the live update changes as replies start and end, silently.
        manager.createNotificationChannel(NotificationChannel(LIVE_CHANNEL, "正在回复", NotificationManager.IMPORTANCE_LOW).apply {
            description = "回复进行中时常驻，显示在状态栏"
        })
        manager.createNotificationChannel(NotificationChannel(ALERT_CHANNEL, "回复提醒", NotificationManager.IMPORTANCE_HIGH).apply {
            description = "回复完成，或模型在等你回答；状态栏显示超级岛时改由超级岛提示"
        })
        manager.createNotificationChannel(NotificationChannel(RESULT_CHANNEL, "回复结果", NotificationManager.IMPORTANCE_LOW).apply {
            description = "超级岛提示过的回复结果，留在通知栏，不弹出"
        })
    }
}

/// Keeps the App running while replies run, so its connection to the server (and with it
/// the live update and the alerts) survives the App going to the background.
class ReplyService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Started with startForegroundService: it must go to the foreground even when the
        // replies ended meanwhile, then it stops.
        val notification = ReplyNotifications.live ?: Notification.Builder(this, "replies").setSmallIcon(R.drawable.ic_notification).build()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(ReplyNotifications.LIVE_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        } else {
            startForeground(ReplyNotifications.LIVE_ID, notification)
        }
        if (!ReplyNotifications.active) {
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
        }
        return START_NOT_STICKY
    }
}
