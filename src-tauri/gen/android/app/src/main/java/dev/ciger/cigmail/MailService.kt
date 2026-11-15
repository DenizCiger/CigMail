package dev.ciger.cigmail

import android.app.AlarmManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import android.os.SystemClock
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat

/**
 * Keeps CigMail's process — and with it the Rust mail engine — alive while the app is in the
 * background or swiped away, so new mail still gets a notification. Android requires a
 * foreground service to show an ongoing notification; it sits in its own silent channel that
 * the user can hide.
 */
class MailService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val nm = getSystemService(NotificationManager::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            nm.createNotificationChannel(
                NotificationChannel(CHANNEL, "Background sync", NotificationManager.IMPORTANCE_MIN).apply {
                    description = "Shown while CigMail checks for new mail in the background. You can turn it off."
                    setShowBadge(false)
                }
            )
        }
        val open = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val notification = NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(R.mipmap.ic_launcher_monochrome)
            .setContentTitle("Checking for new mail")
            .setContentIntent(open)
            .setOngoing(true)
            .setSilent(true)
            .setPriority(NotificationCompat.PRIORITY_MIN)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .build()
        val type = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE
        } else {
            0
        }
        ServiceCompat.startForeground(this, NOTIFICATION_ID, notification, type)
        SyncAlarm.schedule(this)
        // If Android kills the process anyway, a restarted service would have no engine to keep
        // alive: the next app launch starts both again.
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        SyncAlarm.cancel(this)
        super.onDestroy()
    }

    companion object {
        private const val CHANNEL = "background_sync"
        private const val NOTIFICATION_ID = 1

        fun start(context: Context) {
            try {
                ContextCompat.startForegroundService(context, Intent(context, MailService::class.java))
            } catch (e: Exception) {
                Log.w("cigmail", "could not start the background service", e)
            }
        }
    }
}

/**
 * A doze-proof nudge. The engine's own timers stall while the CPU sleeps, so every 15 minutes
 * (the most Android allows in doze) this wakes the device, holds it awake for a minute and asks
 * the engine to sync.
 */
class SyncAlarm : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val pm = context.getSystemService(PowerManager::class.java)
        pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "cigmail:sync").acquire(60_000L)
        NativeBridge.wake()
        schedule(context)
    }

    companion object {
        private const val INTERVAL = 15 * 60 * 1000L

        private fun intent(context: Context) = PendingIntent.getBroadcast(
            context,
            0,
            Intent(context, SyncAlarm::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

        fun schedule(context: Context) {
            val am = context.getSystemService(AlarmManager::class.java)
            am.setAndAllowWhileIdle(AlarmManager.ELAPSED_REALTIME_WAKEUP, SystemClock.elapsedRealtime() + INTERVAL, intent(context))
        }

        fun cancel(context: Context) {
            context.getSystemService(AlarmManager::class.java).cancel(intent(context))
        }
    }
}

/** Calls into the Rust side (`background.rs`, `mailto.rs`). */
object NativeBridge {
    @JvmStatic
    private external fun nativeWake()

    @JvmStatic
    private external fun nativeMailto(url: String)

    fun mailto(url: String) {
        try {
            nativeMailto(url)
        } catch (e: UnsatisfiedLinkError) {
            // Not loaded yet; can't happen once the activity is up.
        }
    }

    fun wake() {
        try {
            nativeWake()
        } catch (e: UnsatisfiedLinkError) {
            // The process was started for the alarm alone: there is no engine to wake.
        }
    }
}
