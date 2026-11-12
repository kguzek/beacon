package uk.guzek.beacon

import android.Manifest
import android.app.Application
import android.content.pm.PackageManager
import android.os.Build
import android.os.PowerManager
import androidx.core.content.ContextCompat
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import uk.guzek.beacon.core.Snapshot
import uk.guzek.beacon.core.defaultStatusPageUrl

/** The settings screen's **Test** result for one URL. */
sealed interface TestState {
    data object Idle : TestState

    data class Running(val url: String) : TestState

    data class Passed(val url: String, val snapshot: Snapshot) : TestState

    data class Failed(val url: String, val message: String) : TestState
}

/** System settings Beacon needs but may not have; each missing one gets a shortcut. */
data class SystemAccess(
    val notifications: Boolean = true,
    val liveUpdates: Boolean = true,
    val exactAlarms: Boolean = true,
    val unrestrictedBattery: Boolean = true,
)

class SettingsViewModel(application: Application) : AndroidViewModel(application) {
    private val monitor = application.monitor
    private val power = application.getSystemService(PowerManager::class.java)

    val settings: StateFlow<Settings> =
        monitor.settings.updates.stateIn(
            viewModelScope,
            SharingStarted.WhileSubscribed(5_000),
            monitor.settings.current,
        )

    val status: StateFlow<StatusState> = monitor.repository.state

    /** Only a hint: there's no default page, the user has to choose one. */
    val placeholderUrl: String = defaultStatusPageUrl()

    private val _url = MutableStateFlow(monitor.settings.current.pageUrl.orEmpty())
    val url: StateFlow<String> = _url.asStateFlow()

    private val _test = MutableStateFlow<TestState>(TestState.Idle)
    val test: StateFlow<TestState> = _test.asStateFlow()

    private val _refreshing = MutableStateFlow(false)
    val refreshing: StateFlow<Boolean> = _refreshing.asStateFlow()

    private val _access = MutableStateFlow(SystemAccess())
    val access: StateFlow<SystemAccess> = _access.asStateFlow()

    init {
        // App launch: re-arm the loop and get a fresh result.
        if (monitor.settings.current.pageUrl != null) refresh()
    }

    fun onUrlChange(value: String) {
        _url.value = value
        _test.update { if (it is TestState.Running) it else TestState.Idle }
    }

    fun runTest() {
        val url = _url.value.trim()
        if (url.isEmpty()) return
        _test.value = TestState.Running(url)
        viewModelScope.launch {
            _test.value =
                when (val result = monitor.repository.test(url)) {
                    is StatusState.Loaded -> TestState.Passed(url, result.snapshot)
                    is StatusState.Failed -> TestState.Failed(url, result.message)
                    StatusState.Checking -> TestState.Idle
                }
        }
    }

    /** Whether **Save** applies: the field's URL passed **Test** and differs from the saved one. */
    fun canSave(url: String, test: TestState, settings: Settings): Boolean =
        test is TestState.Passed && test.url == url.trim() && test.url != settings.pageUrl

    /** Saves the tested URL and starts (or restarts) the tick loop. */
    fun save() {
        val passed = _test.value as? TestState.Passed ?: return
        if (passed.url != _url.value.trim()) return
        monitor.settings.update { it.copy(pageUrl = passed.url) }
        _url.value = passed.url
        monitor.repository.reset()
        refresh()
    }

    fun setInterval(seconds: Int) {
        monitor.settings.update { it.copy(intervalSeconds = seconds) }
        monitor.arm()
    }

    fun setChipMode(mode: ChipMode) {
        monitor.settings.update { it.copy(chipMode = mode) }
        monitor.repost()
    }

    fun setHideWhenOnline(hide: Boolean) {
        monitor.settings.update { it.copy(hideWhenOnline = hide) }
        monitor.repost()
    }

    fun refresh() {
        if (_refreshing.value) return
        _refreshing.value = true
        viewModelScope.launch {
            try {
                monitor.refresh()
            } finally {
                monitor.arm()
                _refreshing.value = false
            }
        }
    }

    /** Re-reads permissions and system toggles; call when the screen resumes. */
    fun checkAccess() {
        val context = getApplication<Application>()
        _access.value =
            SystemAccess(
                notifications = monitor.notifier.canPost,
                liveUpdates = monitor.notifier.canPromote,
                exactAlarms = monitor.scheduler.canScheduleExact,
                unrestrictedBattery = power.isIgnoringBatteryOptimizations(context.packageName),
            )
        // A permission may have just been granted.
        monitor.repost()
    }

    /** Android 13+ asks for notification permission at runtime. */
    val needsNotificationPermission: Boolean
        get() =
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
                ContextCompat.checkSelfPermission(
                    getApplication(),
                    Manifest.permission.POST_NOTIFICATIONS,
                ) != PackageManager.PERMISSION_GRANTED
}
