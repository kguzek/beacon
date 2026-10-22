import { describe, expect, it } from "vitest";

import { currentSchema, effectiveStatus, visibleInventory } from "../src/api";
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
