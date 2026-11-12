package uk.guzek.beacon

import uk.guzek.beacon.core.Overview
import uk.guzek.beacon.core.PublisherStatus
import uk.guzek.beacon.core.ServiceProblem
import uk.guzek.beacon.core.ServiceStatus
import uk.guzek.beacon.core.Snapshot

/** Colours from the status page's palette. */
enum class Tone(val argb: Int) {
    HEALTHY(0xFF7DD3B0.toInt()),
    MIXED(0xFFE8C37A.toInt()),
    DOWN(0xFFED7C8B.toInt()),
    UNKNOWN(0xFF8491A7.toInt()),
}

/**
 * One icon per page-level verdict.
 *
 * The status bar renders the small icon as a monochrome mask, so the [small] glyphs differ by shape
 * (Waybar-style). Where colour shows, the [large] glyphs follow the macOS menu-bar app.
 */
enum class StatusIcon(val tone: Tone, val small: Int, val large: Int) {
    ALL_ONLINE(Tone.HEALTHY, R.drawable.ic_status_online, R.drawable.status_online),
    MIXED(Tone.MIXED, R.drawable.ic_status_mixed, R.drawable.status_mixed),
    PARTIAL_OUTAGE(
        Tone.DOWN,
        R.drawable.ic_status_partial_outage,
        R.drawable.status_partial_outage,
    ),
    /** `!` rather than macOS's ✕: in the status bar, ! reads as more severe than ✕. */
    ALL_DOWN(Tone.DOWN, R.drawable.ic_status_all_down, R.drawable.status_all_down),
    /** Incomplete, unavailable or the fetch failed. */
    UNKNOWN(Tone.UNKNOWN, R.drawable.ic_status_unknown, R.drawable.status_unknown),
    /** No result yet. An ellipsis, since a one-colour beacon outline looks too much like online. */
    CHECKING(Tone.UNKNOWN, R.drawable.ic_status_checking, R.drawable.status_checking),
}

/** Everything the notification and settings screen show for a [StatusState]. */
data class StatusPresentation(
    val icon: StatusIcon,
    /** Headline, e.g. `2 services down`. */
    val title: String,
    /** One line under the title. */
    val text: String,
    /** Status-bar chip text on Android 16+; meant to be around 7 characters. */
    val chip: String,
    /** One block per location, for the expanded notification. */
    val details: String,
) {
    companion object {
        const val CHECKING_TITLE = "Checking service status…"
        const val FAILED_TITLE = "Status page unavailable"

        /** Problems listed per location before collapsing into "and N more". */
        const val MAX_PROBLEMS = 6

        fun of(state: StatusState): StatusPresentation =
            when (state) {
                StatusState.Checking ->
                    StatusPresentation(StatusIcon.CHECKING, CHECKING_TITLE, "", "…", "")
                is StatusState.Failed ->
                    StatusPresentation(
                        StatusIcon.UNKNOWN,
                        FAILED_TITLE,
                        state.message,
                        "?",
                        state.message,
                    )
                is StatusState.Loaded -> of(state.snapshot)
            }

        fun of(snapshot: Snapshot): StatusPresentation =
            StatusPresentation(
                icon = icon(snapshot.overview),
                title = snapshot.headline,
                text = snapshot.breakdown,
                chip = chip(snapshot),
                details = details(snapshot.publishers),
            )

        fun icon(overview: Overview): StatusIcon =
            when (overview) {
                Overview.ALL_ONLINE -> StatusIcon.ALL_ONLINE
                Overview.MIXED -> StatusIcon.MIXED
                Overview.PARTIAL_OUTAGE -> StatusIcon.PARTIAL_OUTAGE
                Overview.ALL_DOWN -> StatusIcon.ALL_DOWN
                Overview.INCOMPLETE,
                Overview.UNAVAILABLE -> StatusIcon.UNKNOWN
            }

        /**
         * Partial outage with mixed services too shows only the down count, the more severe.
         * Scope-wide mixed (one location confirms everything, another is unreachable) has no mixed
         * services, so falls back to plain `Mixed`.
         */
        fun chip(snapshot: Snapshot): String {
            val counts = snapshot.counts
            return when (snapshot.overview) {
                Overview.ALL_ONLINE -> "OK"
                Overview.MIXED -> if (counts.mixed > 0u) "${counts.mixed} mixed" else "Mixed"
                Overview.PARTIAL_OUTAGE -> "${counts.down} down"
                Overview.ALL_DOWN -> "All down"
                Overview.INCOMPLETE -> if (counts.unknown > 0u) "${counts.unknown} ?" else "?"
                Overview.UNAVAILABLE -> "?"
            }
        }

        fun details(publishers: List<PublisherStatus>): String =
            publishers.joinToString("\n") { publisher ->
                buildString {
                    append(publisher.name)
                    append(": ")
                    append(PublisherPresentation.summary(publisher))
                    for (problem in publisher.problems.take(MAX_PROBLEMS)) {
                        append("\n  ")
                        append(PublisherPresentation.problem(problem))
                    }
                    val hidden = publisher.problems.size - MAX_PROBLEMS
                    if (hidden > 0) append("\n  and $hidden more")
                }
            }
    }
}

/** Per-location lines, as in the macOS popover. */
object PublisherPresentation {
    /** e.g. `38 online · 2 down`, or why there is nothing to count. */
    fun summary(publisher: PublisherStatus): String {
        if (!publisher.reachable) return "Unreachable"
        val parts =
            listOf(
                    publisher.healthy to "online",
                    publisher.down to "down",
                    publisher.unknown to "unknown",
                )
                .filter { (count, _) -> count > 0u }
                .map { (count, label) -> "$count $label" }
        return if (parts.isEmpty()) "No services" else parts.joinToString(" · ")
    }

    fun tone(publisher: PublisherStatus): Tone =
        when {
            !publisher.reachable -> Tone.UNKNOWN
            publisher.down > 0u -> Tone.DOWN
            publisher.unknown > 0u -> Tone.UNKNOWN
            else -> Tone.HEALTHY
        }

    fun problem(problem: ServiceProblem): String =
        when (problem.status) {
            ServiceStatus.DOWN -> "${problem.name} — down"
            ServiceStatus.UNKNOWN,
            ServiceStatus.HEALTHY -> "${problem.name} — no recent result"
        }

    fun tone(problem: ServiceProblem): Tone =
        if (problem.status == ServiceStatus.DOWN) Tone.DOWN else Tone.UNKNOWN
}
