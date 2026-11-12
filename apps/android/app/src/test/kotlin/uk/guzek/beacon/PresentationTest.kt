package uk.guzek.beacon

import org.junit.Assert.assertEquals
import org.junit.Test
import uk.guzek.beacon.core.Counts
import uk.guzek.beacon.core.Overview
import uk.guzek.beacon.core.ServiceProblem
import uk.guzek.beacon.core.ServiceStatus

class PresentationTest {
    private fun counts(mixed: UInt = 0u, down: UInt = 0u, unknown: UInt = 0u) =
        Counts(healthy = 3u, mixed = mixed, down = down, unknown = unknown)

    private fun chip(overview: Overview, counts: Counts = counts()) =
        StatusPresentation.of(snapshot(overview, counts)).chip

    @Test
    fun iconFollowsOverview() {
        val icons = Overview.entries.associateWith { StatusPresentation.icon(it) }
        assertEquals(
            mapOf(
                Overview.ALL_ONLINE to StatusIcon.ALL_ONLINE,
                Overview.MIXED to StatusIcon.MIXED,
                Overview.PARTIAL_OUTAGE to StatusIcon.PARTIAL_OUTAGE,
                Overview.ALL_DOWN to StatusIcon.ALL_DOWN,
                Overview.INCOMPLETE to StatusIcon.UNKNOWN,
                Overview.UNAVAILABLE to StatusIcon.UNKNOWN,
            ),
            icons,
        )
        assertEquals(StatusIcon.CHECKING, StatusPresentation.of(StatusState.Checking).icon)
        assertEquals(StatusIcon.UNKNOWN, StatusPresentation.of(StatusState.Failed("x", 0)).icon)
    }

    @Test
    fun tonesMatchThePage() {
        assertEquals(Tone.HEALTHY, StatusIcon.ALL_ONLINE.tone)
        assertEquals(Tone.MIXED, StatusIcon.MIXED.tone)
        assertEquals(Tone.DOWN, StatusIcon.PARTIAL_OUTAGE.tone)
        assertEquals(Tone.DOWN, StatusIcon.ALL_DOWN.tone)
        assertEquals(Tone.UNKNOWN, StatusIcon.UNKNOWN.tone)
        assertEquals(Tone.UNKNOWN, StatusIcon.CHECKING.tone)
        assertEquals(0xFF7DD3B0.toInt(), Tone.HEALTHY.argb)
    }

    @Test
    fun smallIconsDifferByShape() {
        // The status bar drops colour, so no two verdicts may share a glyph.
        val glyphs = StatusIcon.entries.map { it.small }
        assertEquals(glyphs.size, glyphs.toSet().size)
    }

    @Test
    fun outagesShareTheRedCross() {
        // In colour, partial outage and all down both show ✕, as on macOS.
        assertEquals(R.drawable.status_down, StatusIcon.PARTIAL_OUTAGE.large)
        assertEquals(R.drawable.status_down, StatusIcon.ALL_DOWN.large)
    }

    @Test
    fun chipText() {
        assertEquals("OK", chip(Overview.ALL_ONLINE))
        assertEquals("2 mixed", chip(Overview.MIXED, counts(mixed = 2u)))
        // Scope-wide mixed: no single service is mixed.
        assertEquals("Mixed", chip(Overview.MIXED))
        assertEquals("3 down", chip(Overview.PARTIAL_OUTAGE, counts(down = 3u)))
        // Down is more severe than mixed, so it alone is shown.
        assertEquals("1 down", chip(Overview.PARTIAL_OUTAGE, counts(mixed = 4u, down = 1u)))
        assertEquals("All down", chip(Overview.ALL_DOWN, counts(down = 9u)))
        assertEquals("5 ?", chip(Overview.INCOMPLETE, counts(unknown = 5u)))
        assertEquals("?", chip(Overview.INCOMPLETE))
        assertEquals("?", chip(Overview.UNAVAILABLE))
        assertEquals("…", StatusPresentation.of(StatusState.Checking).chip)
        assertEquals("?", StatusPresentation.of(StatusState.Failed("offline", 0)).chip)
    }

    @Test
    fun chipsStayShort() {
        // setShortCriticalText truncates beyond about 7 characters; "All down" is 8.
        for (overview in Overview.entries) {
            val text = chip(overview, counts(mixed = 99u, down = 99u, unknown = 99u))
            assert(text.length <= 8) { "$overview: $text" }
        }
    }

    @Test
    fun snapshotTextComesFromTheCore() {
        val presentation =
            StatusPresentation.of(
                snapshot(
                    Overview.PARTIAL_OUTAGE,
                    headline = "2 services down",
                    breakdown = "38 online",
                )
            )
        assertEquals("2 services down", presentation.title)
        assertEquals("38 online", presentation.text)
    }

    @Test
    fun failureShowsTheError() {
        val presentation = StatusPresentation.of(StatusState.Failed("connection refused", 0))
        assertEquals(StatusPresentation.FAILED_TITLE, presentation.title)
        assertEquals("connection refused", presentation.text)
    }

    @Test
    fun publisherSummary() {
        assertEquals(
            "38 online · 2 down",
            PublisherPresentation.summary(publisher(healthy = 38u, down = 2u)),
        )
        assertEquals("Unreachable", PublisherPresentation.summary(publisher(reachable = false)))
        assertEquals("No services", PublisherPresentation.summary(publisher()))
        assertEquals(
            Tone.UNKNOWN,
            PublisherPresentation.tone(publisher(healthy = 1u, unknown = 1u)),
        )
        assertEquals(Tone.DOWN, PublisherPresentation.tone(publisher(healthy = 1u, down = 1u)))
        assertEquals(Tone.UNKNOWN, PublisherPresentation.tone(publisher(reachable = false)))
        assertEquals(Tone.HEALTHY, PublisherPresentation.tone(publisher(healthy = 1u)))
    }

    @Test
    fun problems() {
        val down = ServiceProblem("git", "Git", ServiceStatus.DOWN)
        val stale = ServiceProblem("mail", "Mail", ServiceStatus.UNKNOWN)
        assertEquals("Git — down", PublisherPresentation.problem(down))
        assertEquals("Mail — no recent result", PublisherPresentation.problem(stale))
        assertEquals(Tone.DOWN, PublisherPresentation.tone(down))
        assertEquals(Tone.UNKNOWN, PublisherPresentation.tone(stale))
    }

    @Test
    fun detailsListEachLocationAndCapProblems() {
        val problems = (1..8).map { ServiceProblem("s$it", "Service $it", ServiceStatus.DOWN) }
        val details =
            StatusPresentation.details(
                listOf(
                    publisher("HP", healthy = 30u, down = 8u, problems = problems),
                    publisher("Pi", reachable = false),
                )
            )
        val expected =
            listOf("HP: 30 online · 8 down") +
                (1..6).map { "  Service $it — down" } +
                listOf("  and 2 more", "Pi: Unreachable")
        assertEquals(expected.joinToString("\n"), details)
    }
}
