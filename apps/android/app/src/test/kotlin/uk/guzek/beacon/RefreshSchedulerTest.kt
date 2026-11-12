package uk.guzek.beacon

import org.junit.Assert.assertEquals
import org.junit.Test

class RefreshSchedulerTest {
    @Test
    fun screenOnUsesTheSetting() {
        for (seconds in Settings.INTERVAL_CHOICES) {
            assertEquals(seconds, RefreshScheduler.nextDelaySeconds(seconds, interactive = true))
        }
    }

    @Test
    fun screenOffWaitsAtLeastFiveMinutes() {
        assertEquals(300, RefreshScheduler.nextDelaySeconds(30, interactive = false))
        assertEquals(300, RefreshScheduler.nextDelaySeconds(60, interactive = false))
        assertEquals(300, RefreshScheduler.nextDelaySeconds(300, interactive = false))
    }

    @Test
    fun unknownChipModeFallsBackToText() {
        assertEquals(ChipMode.TEXT, ChipMode.fromKey(null))
        assertEquals(ChipMode.TEXT, ChipMode.fromKey("nonsense"))
        assertEquals(ChipMode.ICON, ChipMode.fromKey("icon"))
        assertEquals(ChipMode.OFF, ChipMode.fromKey("off"))
    }
}
