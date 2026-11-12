package uk.guzek.beacon

import uk.guzek.beacon.core.Counts
import uk.guzek.beacon.core.HealthState
import uk.guzek.beacon.core.Overview
import uk.guzek.beacon.core.PublisherStatus
import uk.guzek.beacon.core.ServiceProblem
import uk.guzek.beacon.core.Snapshot

fun publisher(
    name: String = "HP",
    reachable: Boolean = true,
    healthy: UInt = 0u,
    down: UInt = 0u,
    unknown: UInt = 0u,
    problems: List<ServiceProblem> = emptyList(),
) =
    PublisherStatus(
        id = name.lowercase(),
        name = name,
        url = "https://beacon-${name.lowercase()}.example.org",
        reachable = reachable,
        generatedAt = if (reachable) 100.0 else null,
        healthy = healthy,
        down = down,
        unknown = unknown,
        problems = problems,
    )

fun snapshot(
    overview: Overview,
    counts: Counts = Counts(healthy = 0u, mixed = 0u, down = 0u, unknown = 0u),
    publishers: List<PublisherStatus> = emptyList(),
    headline: String = "headline",
    breakdown: String = "breakdown",
) =
    Snapshot(
        title = "Status",
        pageUrl = "https://status.example.org",
        checkedAt = 1_700_000_000.0,
        overview = overview,
        state = HealthState.UNKNOWN,
        headline = headline,
        breakdown = breakdown,
        counts = counts,
        services = emptyList(),
        publishers = publishers,
    )
