import { describe, expect, it } from "vitest";

import {
  combinedStatus,
  currentSchema,
  effectiveStatus,
  filterServices,
  healthSummary,
  historySchema,
  mergeSegments,
  normalizeHistory,
  probeSlots,
  serviceGroups,
  visibleInventory,
} from "../src/api";
import type { Current, Observer } from "../src/api";

function snapshot(id = "primary"): Current {
  return {
    schema_version: 1,
    location: { id, name: id },
    generated_at: 100,
    expires_at: 200,
    services: [
      {
        id: "example.org",
        name: "Example",
        hidden: false,
        hosting_location: "secondary",
        status: "healthy",
        observed_at: 90,
        expires_at: 190,
        duration_seconds: 0.1,
      },
      {
        id: "hidden.org",
        name: "Hidden",
        hidden: true,
        hosting_location: null,
        status: "down",
        observed_at: 90,
        expires_at: 190,
        duration_seconds: 0.1,
      },
    ],
  };
}
function observer(id: string, current: Current | null): Observer {
  return {
    config: { id, name: id, url: "https://example.org" },
    current,
    reachable: current !== null,
    loading: false,
    hour: null,
    day: null,
    month: null,
  };
}
describe("public contract and presentation", () => {
  it("validates VPN metadata and retains the badge across observer skew without hiding services", () => {
    const legacy = snapshot();
    expect(currentSchema.safeParse(legacy).success).toBe(true);
    const vpn = snapshot("secondary");
    vpn.services[0]!.requires_vpn = true;
    expect(currentSchema.parse(vpn).services[0]!.requires_vpn).toBe(true);
    expect(
      currentSchema.safeParse({
        ...vpn,
        services: [{ ...vpn.services[0], requires_vpn: "true" }],
      }).success,
    ).toBe(false);
    const observers = [observer("primary", legacy), observer("secondary", vpn)];
    for (const order of [observers, [...observers].reverse()]) {
      expect(visibleInventory(order)).toHaveLength(1);
      expect(visibleInventory(order)[0]!.requires_vpn).toBe(true);
      expect(combinedStatus("example.org", order, 150)).toBe("healthy");
    }
  });
  it("supports legacy metadata and validates bounded public labels in both contracts", () => {
    const value = snapshot();
    for (const labels of [{ group: " Observability " }, {}, undefined]) {
      value.services[0]!.labels = labels;
      expect(currentSchema.safeParse(value).success).toBe(true);
    }
    for (const labels of [
      { group: " " },
      { Group: "X" },
      { group: "a\u0000b" },
      { group: "é".repeat(101) },
      Object.fromEntries(
        Array.from({ length: 17 }, (_, i) => [`key${i}`, "X"]),
      ),
    ]) {
      value.services[0]!.labels = labels;
      expect(currentSchema.safeParse(value).success).toBe(false);
      expect(
        historySchema.safeParse({
          ...value,
          window_seconds: 3600,
          bucket_seconds: 30,
          services: [
            {
              ...value.services[0],
              observed_success_ratio: null,
              coverage: 0,
              buckets: [],
            },
          ],
        }).success,
      ).toBe(false);
    }
  });
  it("aggregates every configured observer, marking disagreement between locations as mixed", () => {
    const first = observer("primary", snapshot());
    const second = observer("secondary", snapshot("secondary"));
    const status = (now = 150) =>
      combinedStatus("example.org", [first, second], now);
    expect(status()).toBe("healthy");
    second.current!.services[0]!.status = "down";
    expect(status()).toBe("mixed");
    second.current!.services[0]!.status = "unknown";
    expect(status()).toBe("mixed");
    second.current!.services[0]!.status = "healthy";
    second.reachable = false;
    expect(status()).toBe("mixed");
    second.loading = true;
    expect(status()).toBe("healthy");
    second.loading = false;
    second.reachable = true;
    first.current!.services[0]!.status = "down";
    second.current!.services[0]!.status = "down";
    expect(status()).toBe("down");
    expect(status(191)).toBe("unknown");
    second.current = null;
    second.reachable = false;
    expect(status()).toBe("down");
    expect(combinedStatus("example.org", [], 150)).toBe("unknown");
  });
  it("distinguishes empty, unknown, incomplete and outage summaries without hidden services", () => {
    const current = snapshot();
    const observers = [observer("primary", current)];
    const summary = () =>
      healthSummary(visibleInventory(observers), observers, 150);
    expect(summary().headline).toBe("All services online");
    expect(summary().breakdown).toBe("1 online");
    expect(summary().descriptions.healthy).toBe("");
    expect(summary().description).not.toContain("Example");
    current.services[0]!.status = "down";
    expect(summary().headline).toBe("All services down");
    current.services.push({
      ...current.services[0]!,
      id: "second.org",
      status: "unknown",
    });
    expect(summary()).toMatchObject({
      headline: "Partial outage",
      down: 1,
      unknown: 1,
      breakdown: "1 down · 1 unknown",
    });
    current.services[0]!.status = "healthy";
    expect(summary().headline).toBe("Service status incomplete");
    current.services[0]!.status = "unknown";
    expect(summary().headline).toBe("Service status unavailable");
    expect(healthSummary([], observers, 150).headline).toBe(
      "Service status unavailable",
    );
  });
  it("reports mixed status when one location is all healthy and another is not", () => {
    const primary = snapshot();
    const secondary = snapshot("secondary");
    const observers = [
      observer("primary", primary),
      observer("secondary", secondary),
    ];
    const summary = () =>
      healthSummary(visibleInventory(observers), observers, 150).headline;
    expect(summary()).toBe("All services online");
    secondary.services[0]!.status = "down";
    expect(summary()).toBe("Service status mixed");
    secondary.services[0]!.status = "unknown";
    expect(summary()).toBe("Service status mixed");
    secondary.services[0]!.status = "healthy";
    observers[1]!.reachable = false;
    expect(summary()).toBe("Service status mixed");
    expect(
      healthSummary(visibleInventory(observers), observers, 150).descriptions
        .mixed,
    ).toBe(
      "Service Example is observed healthy by primary but its status is unavailable at secondary.",
    );
    primary.services[0]!.status = "down";
    expect(summary()).toBe("All services down");
  });
  it("counts services that only some locations confirm as mixed, per group", () => {
    const primary = snapshot();
    const secondary = snapshot("secondary");
    for (const current of [primary, secondary])
      current.services.push({
        ...current.services[0]!,
        id: "second.org",
        name: "Second",
      });
    primary.services[0]!.status = "down";
    secondary.services[2]!.status = "down";
    const observers = [
      observer("primary", primary),
      observer("secondary", secondary),
    ];
    const services = visibleInventory(observers);
    expect(healthSummary(services, observers, 150)).toMatchObject({
      headline: "Service status mixed",
      isMixed: true,
      mixed: 2,
      breakdown: "2 mixed",
      descriptions: {
        mixed:
          "All services are observed healthy by some locations but their status is unhealthy at others.",
      },
    });
    const second = services.filter((service) => service.id === "second.org");
    expect(healthSummary(second, observers, 150)).toMatchObject({
      headline: "Service status mixed",
      mixed: 1,
      descriptions: {
        mixed:
          "Service Second is observed healthy by primary but its status is unhealthy at secondary.",
      },
    });
  });
  it("names every service in each state, including plural lists, without hidden services", () => {
    const current = snapshot();
    current.services[0]!.name = "Navidrome";
    current.services[0]!.status = "down";
    current.services.push(
      { ...current.services[0]!, id: "jellyfin.test", name: "Jellyfin" },
      {
        ...current.services[0]!,
        id: "kuma.test",
        name: "Uptime Kuma (WCSS)",
        status: "unknown",
      },
    );
    const observers = [observer("primary", current)];
    const result = healthSummary(visibleInventory(observers), observers, 150);
    expect(result.breakdown).toBe("2 down · 1 unknown");
    expect(result.descriptions.down).toBe(
      "Services Jellyfin and Navidrome are currently down.",
    );
    expect(result.descriptions.unknown).toBe(
      "Service Uptime Kuma (WCSS) has an unknown status.",
    );
    expect(result.description).not.toContain("Hidden");
    expect(healthSummary([], observers, 150).breakdown).toBe("");
  });
  it("resolves skew by configured priority, not missing legacy labels, without mutating snapshots", () => {
    const first = snapshot();
    const second = snapshot("secondary");
    second.services[0]!.name = "Different name";
    second.services[0]!.labels = { group: "Observability" };
    const observers = [
      observer("primary", first),
      observer("secondary", second),
    ];
    expect(visibleInventory(observers)[0]).toMatchObject({
      name: "Example",
      labels: { group: "Observability" },
    });
    expect(first.services[0]!.labels).toBeUndefined();
    first.services[0]!.labels = { group: "Websites" };
    expect(visibleInventory(observers)[0]!.labels!.group).toBe("Websites");
  });
  it("filters only eligible names/hostnames and keeps full groups and distinct fallback identities", () => {
    const current = snapshot();
    current.services[0]!.labels = { group: "Other services" };
    current.services.push({
      ...current.services[0]!,
      id: "z.test",
      name: "Zed",
      labels: {},
    });
    const services = visibleInventory([observer("primary", current)]);
    expect(filterServices(services, " EXAMPLE.ORG ")).toHaveLength(1);
    expect(filterServices(services, "hidden")).toHaveLength(0);
    expect(filterServices(services, "Other services")).toHaveLength(0);
    expect(serviceGroups(services, "").groups.map((g) => g.id)).toEqual([
      "named:Other services",
      "fallback",
    ]);
    current.services[2]!.labels = { group: "Other services" };
    const groups = serviceGroups(
      visibleInventory([observer("primary", current)]),
      "Example",
    );
    expect(groups.groups[0]!.services).toHaveLength(2);
    expect(groups.groups[0]!.matches).toHaveLength(1);
    expect(
      serviceGroups(
        [current.services[2]!].map((s) => ({ ...s, labels: undefined })),
        "",
      ).grouped,
    ).toBe(false);
  });
  it("validates snapshot shape and rejects duplicate IDs", () => {
    expect(currentSchema.safeParse(snapshot()).success).toBe(true);
    expect(
      currentSchema.safeParse({ ...snapshot(), schema_version: 2 }).success,
    ).toBe(false);
    const value = snapshot();
    value.services.push(value.services[0]!);
    expect(currentSchema.safeParse(value).success).toBe(false);
  });
  it("expires both healthy and down observations", () => {
    const value = snapshot();
    const service = value.services[0]!;
    expect(effectiveStatus(service, value, 150)).toBe("healthy");
    expect(effectiveStatus(service, value, 191)).toBe("unknown");
    service.status = "down";
    expect(effectiveStatus(service, value, 150)).toBe("down");
    expect(effectiveStatus(service, value, 201)).toBe("unknown");
    service.observed_at = 300;
    expect(effectiveStatus(service, value, 150)).toBe("unknown");
    expect(effectiveStatus(undefined, value, 150)).toBe("unknown");
  });
  it("unions skewed inventories and never renders hidden services", () => {
    const first = snapshot();
    const second = snapshot("secondary");
    second.services = [];
    expect(
      visibleInventory([
        observer("primary", first),
        observer("secondary", second),
      ]).map((s) => s.id),
    ).toEqual(["example.org"]);
    expect(
      visibleInventory([
        observer("primary", first),
        observer("secondary", null),
      ]),
    ).toHaveLength(1);
    second.services = [{ ...first.services[0]!, hidden: true }];
    expect(
      visibleInventory([
        observer("primary", first),
        observer("secondary", second),
      ]),
    ).toHaveLength(0);
    expect(
      visibleInventory([
        observer("primary", null),
        observer("secondary", null),
      ]),
    ).toHaveLength(0);
  });
});

describe("history snapshots", () => {
  const service = {
    id: "example.org",
    name: "Example",
    hidden: false,
    hosting_location: null,
    observed_success_ratio: 0.5,
    coverage: 1,
  };
  const envelope = {
    schema_version: 2,
    location: { id: "primary", name: "Primary" },
    generated_at: 3600,
    expires_at: 3700,
    window_seconds: 3600,
    scrape_interval_seconds: 15,
    refresh_seconds: 15,
  };
  it("accepts per-probe and timeline histories", () => {
    const probes = historySchema.parse({
      ...envelope,
      kind: "probes",
      services: [
        {
          ...service,
          probes: [
            { at: 15, success: true, http_status_code: 200 },
            { at: 30, success: false, http_status_code: null },
          ],
        },
      ],
    });
    expect(normalizeHistory(probes)).toBe(probes);
    const timeline = historySchema.parse({
      ...envelope,
      kind: "timeline",
      step_seconds: 60,
      services: [
        {
          ...service,
          segments: [
            {
              start: 0,
              end: 3600,
              state: "down",
              samples: 240,
              failures: 1,
              last_http_status_code: 503,
            },
          ],
        },
      ],
    });
    expect(normalizeHistory(timeline).kind).toBe("timeline");
    expect(
      historySchema.safeParse({
        ...envelope,
        kind: "probes",
        services: [{ ...service, segments: [] }],
      }).success,
    ).toBe(false);
  });
  it("maps legacy bucket histories onto merged timeline segments", () => {
    const legacy = historySchema.parse({
      schema_version: 1,
      location: { id: "primary", name: "Primary" },
      generated_at: 120,
      expires_at: 200,
      window_seconds: 86_400,
      bucket_seconds: 30,
      services: [
        {
          ...service,
          buckets: [
            {
              ends_at: 30,
              observed_success_ratio: 1,
              coverage: 1,
              samples: 2,
              last_http_status_code: 200,
            },
            { ends_at: 60, observed_success_ratio: 1, coverage: 1, samples: 2 },
            {
              ends_at: 90,
              observed_success_ratio: 0.5,
              coverage: 1,
              samples: 2,
              last_http_status_code: 503,
            },
            {
              ends_at: 120,
              observed_success_ratio: null,
              coverage: 0,
              samples: 0,
            },
          ],
        },
      ],
    });
    const history = normalizeHistory(legacy);
    expect(history.kind).toBe("timeline");
    expect(history.refresh_seconds).toBe(300);
    const segments = history.services[0]!;
    expect("segments" in segments && segments.segments).toEqual([
      {
        start: 0,
        end: 60,
        state: "healthy",
        samples: 4,
        failures: 0,
        last_http_status_code: 200,
      },
      {
        start: 60,
        end: 90,
        state: "down",
        samples: 2,
        failures: 1,
        last_http_status_code: 503,
      },
      {
        start: 90,
        end: 120,
        state: "unknown",
        samples: 0,
        failures: 0,
        last_http_status_code: null,
      },
    ]);
  });
  it("never merges segments across a gap", () => {
    const segment = {
      state: "healthy" as const,
      samples: 1,
      failures: 0,
      last_http_status_code: null,
    };
    expect(
      mergeSegments([
        { ...segment, start: 0, end: 10 },
        { ...segment, start: 20, end: 30 },
      ]),
    ).toHaveLength(2);
  });
  it("keeps one slot per expected check, marking missed probes", () => {
    const probe = (at: number) => ({
      at,
      success: true,
      http_status_code: 200,
    });
    const all = Array.from({ length: 240 }, (_, index) =>
      probe(15 * (index + 1)),
    );
    expect(probeSlots(all, 0, 3600, 15)).toHaveLength(240);
    // Small jitter does not invent missed checks.
    const jittered = all.map((p, index) => ({
      ...p,
      at: p.at + (index % 2 ? 2 : -2),
    }));
    expect(
      probeSlots(jittered, 0, 3600, 15).every((slot) => slot.kind === "probe"),
    ).toBe(true);
    // Two consecutive scrapes missing in the middle. At the end, a check is
    // only missed once overdue: one interval of silence is not yet a gap.
    const lastMissing = all.filter((p) => p.at !== 3600);
    expect(probeSlots(lastMissing, 0, 3600, 15)).toHaveLength(239);
    const gappy = all.filter((p) => ![600, 615, 3585, 3600].includes(p.at));
    const slots = probeSlots(gappy, 0, 3600, 15);
    expect(slots).toHaveLength(239);
    expect(
      slots.filter((slot) => slot.kind === "missed").map((slot) => slot.at),
    ).toEqual([600, 615, 3585]);
    // Probes outside the window are ignored; empty history is all missed.
    expect(
      probeSlots([probe(0), probe(4000)], 0, 3600, 15).every(
        (slot) => slot.kind === "missed",
      ),
    ).toBe(true);
  });
});
