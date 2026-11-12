package uk.guzek.beacon

import android.app.Application
import android.content.Context
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob

class BeaconApplication : Application() {
    /** Outlives receivers and screens, so a shared fetch isn't cancelled with its first caller. */
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    lateinit var monitor: StatusMonitor
        private set

    override fun onCreate() {
        super.onCreate()
        monitor =
            StatusMonitor(
                settings = SettingsStore(this),
                repository = StatusRepository(scope),
                notifier = StatusNotifier(this).apply { createChannel() },
                scheduler = RefreshScheduler(this),
            )
    }
}

val Context.monitor: StatusMonitor
    get() = (applicationContext as BeaconApplication).monitor

/** Ties settings, fetching, the notification and scheduling together. */
class StatusMonitor(
    val settings: SettingsStore,
    val repository: StatusRepository,
    val notifier: StatusNotifier,
    val scheduler: RefreshScheduler,
) {
    /** Fetches, then updates the notification. Does nothing until a page URL is saved. */
    suspend fun refresh() {
        val current = settings.current
        val pageUrl = current.pageUrl ?: return
        // Show "checking" until the first result after a process start.
        if (repository.state.value == StatusState.Checking) {
            notifier.update(StatusState.Checking, current)
        }
        val state = repository.refresh(pageUrl)
        settings.lastRefreshMillis = System.currentTimeMillis()
        // Settings may have changed during the fetch.
        notifier.update(state, settings.current)
    }

    /** Arms the tick loop and safety net. Does nothing until a page URL is saved. */
    fun arm() {
        val current = settings.current
        if (current.pageUrl == null) return stop()
        scheduler.scheduleNext(current)
        scheduler.ensureSafetyNet()
    }

    fun stop() {
        scheduler.stop()
        notifier.cancel()
    }

    /** Re-posts the notification from the last result, e.g. after a chip-mode change. */
    fun repost() {
        notifier.update(repository.state.value, settings.current)
    }
}
