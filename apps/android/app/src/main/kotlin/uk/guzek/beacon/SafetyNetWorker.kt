package uk.guzek.beacon

import android.content.Context
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters

/**
 * Runs every 15 minutes and restarts the alarm chain if it was lost (app update, OEM task killer,
 * recovery from force-stop).
 */
class SafetyNetWorker(context: Context, params: WorkerParameters) :
    CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val monitor = applicationContext.monitor
        val settings = monitor.settings.current
        if (settings.pageUrl == null) return Result.success()

        // Doze throttles while-idle alarms to about one per 9–15 min, so only
        // treat the chain as dead well past that.
        val stale =
            System.currentTimeMillis() - monitor.settings.lastRefreshMillis >
                STALE_AFTER_MINUTES * 60_000
        if (!monitor.scheduler.isScheduled || stale) {
            monitor.refresh()
            monitor.arm()
        }
        return Result.success()
    }

    private companion object {
        const val STALE_AFTER_MINUTES = 20L
    }
}
