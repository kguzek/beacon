package uk.guzek.beacon

import android.content.Context
import android.content.SharedPreferences
import androidx.core.content.edit
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.flow.conflate
import kotlinx.coroutines.flow.distinctUntilChanged

/** How the promoted notification's status-bar chip looks. */
enum class ChipMode(val key: String) {
    /** Glyph and short text, e.g. `2 down`. */
    TEXT("text"),
    /** Glyph only. */
    ICON("icon"),
    /** No promotion: just the glyph among the ordinary notification icons. */
    OFF("off");

    companion object {
        fun fromKey(key: String?): ChipMode = entries.firstOrNull { it.key == key } ?: TEXT
    }
}

data class Settings(
    /** Unset until the user saves a URL that passed **Test**. */
    val pageUrl: String? = null,
    val intervalSeconds: Int = DEFAULT_INTERVAL_SECONDS,
    val chipMode: ChipMode = ChipMode.TEXT,
    val hideWhenOnline: Boolean = false,
) {
    companion object {
        const val DEFAULT_INTERVAL_SECONDS = 60
        val INTERVAL_CHOICES = listOf(30, 60, 300)
    }
}

/**
 * Settings in [SharedPreferences], because receivers read them synchronously and there are only a
 * few keys.
 */
class SettingsStore(context: Context) {
    private val prefs = context.getSharedPreferences("settings", Context.MODE_PRIVATE)
    /** Bookkeeping, kept apart so writing it doesn't look like a settings change. */
    private val state = context.getSharedPreferences("state", Context.MODE_PRIVATE)

    val current: Settings
        get() =
            Settings(
                pageUrl = prefs.getString(KEY_PAGE_URL, null),
                intervalSeconds =
                    prefs.getInt(KEY_INTERVAL_SECONDS, Settings.DEFAULT_INTERVAL_SECONDS).takeIf {
                        it in Settings.INTERVAL_CHOICES
                    } ?: Settings.DEFAULT_INTERVAL_SECONDS,
                chipMode = ChipMode.fromKey(prefs.getString(KEY_CHIP_MODE, null)),
                hideWhenOnline = prefs.getBoolean(KEY_HIDE_WHEN_ONLINE, false),
            )

    /** Emits the current settings, then again after every change. */
    val updates: Flow<Settings> = callbackFlow {
        val listener = SharedPreferences.OnSharedPreferenceChangeListener { _, _ ->
            trySend(current)
        }
        prefs.registerOnSharedPreferenceChangeListener(listener)
        send(current)
        awaitClose { prefs.unregisterOnSharedPreferenceChangeListener(listener) }
    }
        .conflate()
        .distinctUntilChanged()

    fun update(transform: (Settings) -> Settings) {
        val next = transform(current)
        prefs.edit {
            if (next.pageUrl == null) remove(KEY_PAGE_URL)
            else putString(KEY_PAGE_URL, next.pageUrl)
            putInt(KEY_INTERVAL_SECONDS, next.intervalSeconds)
            putString(KEY_CHIP_MODE, next.chipMode.key)
            putBoolean(KEY_HIDE_WHEN_ONLINE, next.hideWhenOnline)
        }
    }

    /** When the last refresh finished, so the safety net can tell whether the alarm chain died. */
    var lastRefreshMillis: Long
        get() = state.getLong(KEY_LAST_REFRESH, 0)
        set(value) = state.edit { putLong(KEY_LAST_REFRESH, value) }

    private companion object {
        const val KEY_PAGE_URL = "page_url"
        const val KEY_INTERVAL_SECONDS = "interval_seconds"
        const val KEY_CHIP_MODE = "chip_mode"
        const val KEY_HIDE_WHEN_ONLINE = "hide_when_online"
        const val KEY_LAST_REFRESH = "last_refresh_millis"
    }
}
