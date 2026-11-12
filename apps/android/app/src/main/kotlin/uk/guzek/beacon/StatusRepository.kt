package uk.guzek.beacon

import kotlin.time.Duration
import kotlin.time.Duration.Companion.seconds
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withTimeout
import uk.guzek.beacon.core.FetchException
import uk.guzek.beacon.core.Snapshot
import uk.guzek.beacon.core.fetchStatus

/** What the app knows about the status page right now. */
sealed interface StatusState {
    /** No result yet since the process started. */
    data object Checking : StatusState

    data class Loaded(val snapshot: Snapshot) : StatusState

    /** The last fetch failed; old results aren't kept, so nothing stale shows as healthy. */
    data class Failed(val message: String, val atMillis: Long) : StatusState
}

/** When this result was checked; null while checking. */
val StatusState.checkedAtMillis: Long?
    get() =
        when (this) {
            StatusState.Checking -> null
            is StatusState.Loaded -> (snapshot.checkedAt * 1000).toLong()
            is StatusState.Failed -> atMillis
        }

/** The Rust core's `fetch_status`, behind an interface so tests can replace it. */
fun interface StatusSource {
    /** Blocks the calling thread. */
    @Throws(FetchException::class) fun fetch(pageUrl: String): Snapshot
}

val RustStatusSource = StatusSource(::fetchStatus)

/**
 * Fetches the status through the Rust core and keeps the latest result in memory only.
 *
 * Alarm ticks, the notification's Refresh action, the settings screen and the safety-net worker can
 * all ask for a refresh. Only one fetch per URL runs at a time: callers that arrive while it runs
 * share its result.
 */
class StatusRepository(
    private val scope: CoroutineScope,
    private val source: StatusSource = RustStatusSource,
    private val io: CoroutineDispatcher = Dispatchers.IO,
    /** `locations.json` and the publishers each have an 8 s timeout in `beacon-core`. */
    private val timeout: Duration = 20.seconds,
    private val clock: () -> Long = System::currentTimeMillis,
) {
    private val _state = MutableStateFlow<StatusState>(StatusState.Checking)
    val state: StateFlow<StatusState> = _state.asStateFlow()

    private val mutex = Mutex()
    private var inFlight: Pair<String, Deferred<StatusState>>? = null

    suspend fun refresh(pageUrl: String): StatusState {
        val fetch = mutex.withLock {
            inFlight?.takeIf { (url, job) -> url == pageUrl && job.isActive }?.second
                ?: scope.async { fetchAndStore(pageUrl) }.also { inFlight = pageUrl to it }
        }
        return fetch.await()
    }

    /** Tries a URL without touching the shared state, for the settings screen's **Test**. */
    suspend fun test(pageUrl: String): StatusState = fetch(pageUrl)

    /** Drops the last result, e.g. after the URL changes. */
    fun reset() {
        _state.value = StatusState.Checking
    }

    private suspend fun fetchAndStore(pageUrl: String): StatusState =
        fetch(pageUrl).also { _state.value = it }

    private suspend fun fetch(pageUrl: String): StatusState {
        // The FFI call blocks and can't be interrupted, so time out the wait
        // rather than the call; an abandoned call ends on Rust's own timeouts.
        // runCatching keeps a failed call from cancelling the shared scope.
        val call = scope.async(io) { runCatching { source.fetch(pageUrl) } }
        val result =
            try {
                withTimeout(timeout) { call.await() }
            } catch (e: TimeoutCancellationException) {
                call.cancel()
                return StatusState.Failed("Timed out after ${timeout.inWholeSeconds} s", clock())
            }
        return result.fold(
            onSuccess = { StatusState.Loaded(it) },
            // FetchException, or InternalException if the Rust side panicked.
            onFailure = { StatusState.Failed(it.message ?: it.toString(), clock()) },
        )
    }
}
