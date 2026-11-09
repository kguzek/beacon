// Shared with crates/beacon-core/tests/conformance.rs: the desktop clients'
// Rust port must agree with the page on these cases.
import { readFileSync, readdirSync } from "node:fs";
import { describe, expect, it } from "vitest";

import {
  combinedStatus,
  currentSchema,
  healthSummary,
  settingsSchema,
  visibleInventory,
} from "../src/api";
import type {
  Current,
  HealthState,
  LocationConfig,
  Observer,
} from "../src/api";

interface Fixture {
  description: string;
  now: number;
  observers: {
    config: LocationConfig;
    reachable: boolean;
    current: Current | null;
  }[];
  expected: {
    services: Record<string, HealthState>;
    headline: string;
    counts: Record<HealthState, number>;
    breakdown: string;
  };
}

const directory = new URL("../../../fixtures/current/", import.meta.url);
const fixtures = readdirSync(directory)
  .filter((name) => name.endsWith(".json"))
  .map(
    (name) =>
      JSON.parse(readFileSync(new URL(name, directory), "utf8")) as Fixture,
  );

describe("shared current-status fixtures", () => {
  it("exist", () => {
    expect(fixtures.length).toBeGreaterThan(0);
  });
  it.each(fixtures.map((fixture) => [fixture.description, fixture] as const))(
    "%s",
    (_, fixture) => {
      settingsSchema.parse({
        locations: fixture.observers.map((observer) => observer.config),
      });
      const observers: Observer[] = fixture.observers.map((observer) => ({
        config: observer.config,
        reachable: observer.reachable,
        current:
          observer.current === null
            ? null
            : currentSchema.parse(observer.current),
        loading: false,
        hour: null,
        day: null,
        month: null,
      }));
      const services = visibleInventory(observers);
      const summary = healthSummary(services, observers, fixture.now);
      expect(
        Object.fromEntries(
          services.map((service) => [
            service.id,
            combinedStatus(service.id, observers, fixture.now),
          ]),
        ),
      ).toEqual(fixture.expected.services);
      expect({
        headline: summary.headline,
        counts: {
          healthy: summary.healthy,
          mixed: summary.mixed,
          down: summary.down,
          unknown: summary.unknown,
        },
        breakdown: summary.breakdown,
      }).toEqual({
        headline: fixture.expected.headline,
        counts: fixture.expected.counts,
        breakdown: fixture.expected.breakdown,
      });
    },
  );
});
