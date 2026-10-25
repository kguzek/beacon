import { describe, expect, it } from "vitest";

import {
  combinedStatus,
  currentSchema,
  effectiveStatus,
  filterServices,
  healthSummary,
  historySchema,
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
  it("aggregates every configured observer, retaining fresh failures and successes during outages", () => {
    const first = observer("primary", snapshot());
    const second = observer("secondary", snapshot("secondary"));
    expect(combinedStatus("example.org", [first, second], 150)).toBe("healthy");
    second.reachable = false;
    expect(combinedStatus("example.org", [first, second], 150)).toBe("healthy");
    second.current!.services[0]!.status = "down";
    expect(combinedStatus("example.org", [first, second], 150)).toBe("down");
    expect(combinedStatus("example.org", [first, second], 191)).toBe("unknown");
    second.current = null;
    expect(combinedStatus("example.org", [first, second], 150)).toBe("unknown");
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
