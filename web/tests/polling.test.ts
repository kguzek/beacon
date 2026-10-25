import { describe, expect, it } from "vitest";

import { SnapshotPollSchedule } from "../src/polling";

const snapshot = (generated_at: number, expires_at = generated_at + 120) => ({
  generated_at,
  expires_at,
});

describe("snapshot-aligned polling", () => {
  it("aligns each publisher to its own generation time instead of page load", () => {
    const hp = new SnapshotPollSchedule(30);
    const pi = new SnapshotPollSchedule(30);
    expect(hp.nextDelay(snapshot(100), 117)).toBe(13_500);
    expect(pi.nextDelay(snapshot(110), 117)).toBe(23_500);
  });

  it("learns generation drift and retains cadence after missed publications", () => {
    const polling = new SnapshotPollSchedule(30);
    polling.nextDelay(snapshot(100), 117);
    expect(polling.nextDelay(snapshot(130.25), 130.75)).toBe(30_250);
    expect(polling.nextDelay(snapshot(190.75), 191.25)).toBe(30_250);
  });

  it("briefly retries an unpublished snapshot without an unbounded retry loop", () => {
    const polling = new SnapshotPollSchedule(30);
    polling.nextDelay(snapshot(100), 117);
    for (let attempt = 0; attempt < 5; attempt++) {
      expect(polling.nextDelay(snapshot(100), 130.5 + attempt)).toBe(1000);
    }
    expect(polling.nextDelay(snapshot(100), 135.5)).toBe(30_000);
    expect(polling.nextDelay(snapshot(130), 136.5)).toBe(24_000);
  });

  it("backs off on failures, expired snapshots, clock skew and regressing data", () => {
    const polling = new SnapshotPollSchedule(30);
    polling.nextDelay(snapshot(100), 117);
    expect(polling.failureDelay()).toBe(30_000);
    expect(polling.nextDelay(snapshot(100), 220)).toBe(30_000);
    expect(polling.nextDelay(snapshot(300), 117)).toBe(30_000);
    expect(polling.nextDelay(snapshot(90), 117)).toBe(30_000);
    expect(polling.nextDelay(snapshot(130), 131)).toBe(29_500);
  });

  it("aligns history windows using their independent refresh cadence", () => {
    const day = new SnapshotPollSchedule(300);
    expect(day.nextDelay(snapshot(100, 1000), 217)).toBe(183_500);
    expect(day.nextDelay(snapshot(400.25, 1300), 401)).toBe(300_000);
  });
});
