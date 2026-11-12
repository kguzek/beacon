package uk.guzek.beacon

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.Context
import android.os.Build
import android.os.PowerManager
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import java.util.concurrent.TimeUnit

/**
 * Drives refreshes with a chain of exact alarms (each tick schedules the next), plus a WorkManager
 * job that re-arms the chain if it was lost. No foreground service.
 */
class RefreshScheduler(private val context: Context) {
    private val alarms = context.getSystemService(AlarmManager::class.java)
    private val power = context.getSystemService(PowerManager::class.java)

    val canScheduleExact: Boolean
        get() = Build.VERSION.SDK_INT < Build.VERSION_CODES.S || alarms.canScheduleExactAlarms()

    /**
     * Whether a tick is pending. Alarms don't survive force-stop or, on some OEMs, task killers.
     */
    val isScheduled: Boolean
        get() = RefreshReceiver.tickIntent(context, PendingIntent.FLAG_NO_CREATE) != null

    /** Schedules the next tick, replacing any pending one. */
    fun scheduleNext(settings: Settings) {
        val delay = nextDelaySeconds(settings.intervalSeconds, power.isInteractive) * 1000L
        val at = System.currentTimeMillis() + delay
        val tick = RefreshReceiver.tickIntent(context, 0) ?: return
        if (canScheduleExact) {
            alarms.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, tick)
        } else {
            alarms.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, tick)
        }
    }

    fun ensureSafetyNet() {
        val request =
            PeriodicWorkRequestBuilder<SafetyNetWorker>(
                    SAFETY_NET_MINUTES,
                    TimeUnit.MINUTES,
                )
                .build()
        WorkManager.getInstance(context)
            .enqueueUniquePeriodicWork(SAFETY_NET, ExistingPeriodicWorkPolicy.KEEP, request)
    }

    fun stop() {
        RefreshReceiver.tickIntent(context, PendingIntent.FLAG_NO_CREATE)?.let {
            alarms.cancel(it)
            it.cancel()
        }
        WorkManager.getInstance(context).cancelUniqueWork(SAFETY_NET)
    }

    companion object {
        /** With the screen off, tick at most this often to avoid waking the radio every minute. */
        const val SCREEN_OFF_INTERVAL_SECONDS = 300
        const val SAFETY_NET_MINUTES = 15L
        private const val SAFETY_NET = "refresh-safety-net"

        fun nextDelaySeconds(intervalSeconds: Int, interactive: Boolean): Int =
            if (interactive) intervalSeconds
            else maxOf(intervalSeconds, SCREEN_OFF_INTERVAL_SECONDS)
    }
}
