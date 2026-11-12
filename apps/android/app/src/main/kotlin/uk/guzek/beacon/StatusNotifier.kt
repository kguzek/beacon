package uk.guzek.beacon

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import androidx.core.graphics.drawable.toBitmap
import androidx.core.net.toUri

/**
 * Posts the ongoing status notification. On Android 16+ it is promoted, which gives it a status-bar
 * chip, without a foreground service: the alarm tick loop re-posts it.
 */
class StatusNotifier(private val context: Context) {
    private val manager = NotificationManagerCompat.from(context)

    fun createChannel() {
        // IMPORTANCE_MIN would hide the status-bar icon; DEFAULT with no sound
        // or vibration is known to allow promotion.
        val channel =
            NotificationChannel(
                    CHANNEL_STATUS,
                    context.getString(R.string.channel_status),
                    NotificationManager.IMPORTANCE_DEFAULT,
                )
                .apply {
                    description = context.getString(R.string.channel_status_description)
                    setSound(null, null)
                    enableVibration(false)
                    setShowBadge(false)
                }
        context.getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    val canPost: Boolean
        get() =
            manager.areNotificationsEnabled() &&
                (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
                    ContextCompat.checkSelfPermission(
                        context,
                        Manifest.permission.POST_NOTIFICATIONS,
                    ) == PackageManager.PERMISSION_GRANTED)

    /** Whether the system lets this app's notifications be promoted (the "Live updates" toggle). */
    val canPromote: Boolean
        get() =
            Build.VERSION.SDK_INT < Build.VERSION_CODES.BAKLAVA ||
                context
                    .getSystemService(NotificationManager::class.java)
                    .canPostPromotedNotifications()

    fun update(state: StatusState, settings: Settings) {
        val configuredUrl = settings.pageUrl ?: return cancel()
        val pageUrl = (state as? StatusState.Loaded)?.snapshot?.pageUrl ?: configuredUrl
        val presentation = StatusPresentation.of(state)
        if (settings.hideWhenOnline && presentation.icon == StatusIcon.ALL_ONLINE) return cancel()
        if (!canPost) return
        // Permission checked through canPost; lint can't see that.
        @Suppress("MissingPermission")
        manager.notify(
            NOTIFICATION_ID,
            build(
                presentation,
                settings,
                pageUrl,
                state.checkedAtMillis ?: System.currentTimeMillis(),
            ),
        )
    }

    fun cancel() = manager.cancel(NOTIFICATION_ID)

    private fun build(
        presentation: StatusPresentation,
        settings: Settings,
        pageUrl: String,
        checkedAtMillis: Long,
    ) =
        NotificationCompat.Builder(context, CHANNEL_STATUS)
            .setSmallIcon(presentation.icon.small)
            .setLargeIcon(largeIcon(presentation.icon))
            // Not colorized: that disqualifies promotion.
            .setColor(presentation.icon.tone.argb)
            .setContentTitle(presentation.title)
            .setContentText(presentation.text.ifEmpty { null })
            .apply {
                if (presentation.details.isNotEmpty()) {
                    // Promotion allows BigTextStyle but not InboxStyle.
                    setStyle(NotificationCompat.BigTextStyle().bigText(presentation.details))
                }
                when (settings.chipMode) {
                    ChipMode.TEXT ->
                        setRequestPromotedOngoing(true).setShortCriticalText(presentation.chip)
                    // No short text and a past `when`, so nothing gives the chip text.
                    ChipMode.ICON -> setRequestPromotedOngoing(true)
                    ChipMode.OFF -> Unit
                }
            }
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setWhen(checkedAtMillis)
            .setShowWhen(true)
            .setCategory(NotificationCompat.CATEGORY_STATUS)
            .setContentIntent(openPage(pageUrl))
            .addAction(
                R.drawable.ic_refresh,
                context.getString(R.string.action_refresh),
                RefreshReceiver.refreshIntent(context),
            )
            .build()

    private fun largeIcon(icon: StatusIcon) =
        ContextCompat.getDrawable(context, icon.large)?.let { drawable ->
            val size =
                context.resources.getDimensionPixelSize(
                    android.R.dimen.notification_large_icon_width
                )
            drawable.toBitmap(size, size)
        }

    /** Opens the page in the browser directly; no trampoline. */
    private fun openPage(pageUrl: String): PendingIntent =
        PendingIntent.getActivity(
            context,
            0,
            Intent(Intent.ACTION_VIEW, pageUrl.toUri()),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

    companion object {
        const val CHANNEL_STATUS = "status"
        private const val NOTIFICATION_ID = 1
    }
}
