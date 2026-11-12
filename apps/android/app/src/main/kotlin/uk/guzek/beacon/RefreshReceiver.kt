package uk.guzek.beacon

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

/** Alarm ticks, the notification's Refresh action, boot, app updates and exact-alarm grants. */
class RefreshReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            ACTION_TICK,
            ACTION_REFRESH,
            Intent.ACTION_BOOT_COMPLETED,
            Intent.ACTION_MY_PACKAGE_REPLACED,
            AlarmManager.ACTION_SCHEDULE_EXACT_ALARM_PERMISSION_STATE_CHANGED -> Unit
            else -> return
        }
        val monitor = context.monitor
        if (monitor.settings.current.pageUrl == null) return

        // Alarm broadcasts run in the background, where the system allows far
        // longer than a fetch's worst case (~16 s, capped at 20 s) before an ANR.
        val pending = goAsync()
        scope.launch {
            try {
                monitor.refresh()
            } catch (e: Exception) {
                Log.w(TAG, "Refresh failed", e)
            } finally {
                // Always schedule the next tick, so one bad refresh can't break the chain.
                monitor.arm()
                pending.finish()
            }
        }
    }

    companion object {
        private const val TAG = "RefreshReceiver"
        private const val ACTION_TICK = "uk.guzek.beacon.action.TICK"
        private const val ACTION_REFRESH = "uk.guzek.beacon.action.REFRESH"

        private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

        /** The alarm's intent; with [PendingIntent.FLAG_NO_CREATE], null when no alarm is set. */
        fun tickIntent(context: Context, flags: Int): PendingIntent? =
            PendingIntent.getBroadcast(
                context,
                REQUEST_TICK,
                Intent(context, RefreshReceiver::class.java).setAction(ACTION_TICK),
                flags or PendingIntent.FLAG_IMMUTABLE,
            )

        fun refreshIntent(context: Context): PendingIntent =
            PendingIntent.getBroadcast(
                context,
                REQUEST_REFRESH,
                Intent(context, RefreshReceiver::class.java).setAction(ACTION_REFRESH),
                PendingIntent.FLAG_IMMUTABLE,
            )

        private const val REQUEST_TICK = 1
        private const val REQUEST_REFRESH = 2
    }
}
