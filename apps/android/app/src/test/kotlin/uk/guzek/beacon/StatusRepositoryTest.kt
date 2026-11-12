package uk.guzek.beacon

import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import kotlin.time.Duration.Companion.milliseconds
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.cancel
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import uk.guzek.beacon.core.FetchException
import uk.guzek.beacon.core.Overview

/**
 * Uses real dispatchers: the repository's timeout must measure wall-clock time while the blocking
 * fetch runs, which a virtual-time test scheduler would skip straight past.
 */
class StatusRepositoryTest {
    private val online = snapshot(Overview.ALL_ONLINE)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    @After fun tearDown() = scope.cancel()

    @Test
    fun concurrentRefreshesShareOneFetch() = runBlocking {
        val calls = AtomicInteger()
        val release = CountDownLatch(1)
        val repository =
            StatusRepository(
                scope = scope,
                source = {
                    calls.incrementAndGet()
                    release.await(5, TimeUnit.SECONDS)
                    online
                },
                io = Dispatchers.IO,
            )
        val results = (1..3).map { async(Dispatchers.Default) { repository.refresh("https://a") } }
        // The first fetch is blocked, so every caller arrives while it runs.
        while (calls.get() == 0) Thread.sleep(10)
        Thread.sleep(100)
        release.countDown()
        assertEquals(List(3) { StatusState.Loaded(online) }, results.awaitAll())
        assertEquals(1, calls.get())
        assertEquals(StatusState.Loaded(online), repository.state.value)
    }

    @Test
    fun failureReplacesOldResult() = runBlocking {
        var fail = false
        val repository =
            StatusRepository(
                scope = scope,
                source = {
                    if (fail) throw FetchException.Request("connection refused") else online
                },
                io = Dispatchers.IO,
                clock = { 42 },
            )
        repository.refresh("https://a")
        fail = true
        val failed = repository.refresh("https://a")
        assertEquals(StatusState.Failed("connection refused", 42), failed)
        assertEquals(failed, repository.state.value)
    }

    @Test
    fun slowFetchTimesOut() = runBlocking {
        val release = CountDownLatch(1)
        val repository =
            StatusRepository(
                scope = scope,
                source = {
                    release.await(5, TimeUnit.SECONDS)
                    online
                },
                io = Dispatchers.IO,
                timeout = 50.milliseconds,
                clock = { 7 },
            )
        val result = repository.refresh("https://a")
        release.countDown()
        assertTrue(result is StatusState.Failed)
        assertEquals(7L, result.checkedAtMillis)
    }

    @Test
    fun testDoesNotTouchSharedState() = runBlocking {
        val repository = StatusRepository(scope, source = { online }, io = Dispatchers.IO)
        assertEquals(StatusState.Loaded(online), repository.test("https://a"))
        assertEquals(StatusState.Checking, repository.state.value)
    }

    @Test
    fun checkedAtComesFromTheSnapshot() {
        assertEquals(1_700_000_000_000L, StatusState.Loaded(online).checkedAtMillis)
        assertEquals(null, StatusState.Checking.checkedAtMillis)
    }

    @Test(timeout = 30_000)
    fun rustCoreRejectsInvalidUrlWithoutNetwork() = runBlocking {
        // Calls the real core through the host library from ./build.sh bindings.
        val repository = StatusRepository(scope, io = Dispatchers.IO)
        val result = repository.test("ftp://example.org")
        assertTrue("$result", result is StatusState.Failed)
    }
}
