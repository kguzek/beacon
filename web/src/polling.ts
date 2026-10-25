/** Track one publisher stream independently of page load and other publishers. */
export class SnapshotPollSchedule {
  private generatedAt: number | undefined;
  private interval: number;
  private retries = 0;

  constructor(private readonly defaultInterval: number) {
    this.interval = defaultInterval;
  }

  nextDelay(
    snapshot: { generated_at: number; expires_at: number },
    now: number,
  ) {
    const generated = snapshot.generated_at;
    if (generated > now + 5 || snapshot.expires_at <= now) {
      return this.failureDelay();
    }
    if (this.generatedAt !== undefined && generated < this.generatedAt) {
      return this.failureDelay();
    }
    if (this.generatedAt !== undefined && generated > this.generatedAt) {
      const elapsed = generated - this.generatedAt;
      // Missed polls may span several publications. Keep the underlying cadence,
      // including query time (publishers sleep after completing their queries).
      const periods = Math.max(1, Math.round(elapsed / this.interval));
      const measured = elapsed / periods;
      if (
        measured >= this.defaultInterval * 0.8 &&
        measured <= this.defaultInterval * 1.5
      ) {
        this.interval = measured;
      }
    }
    if (generated !== this.generatedAt) {
      this.retries = 0;
    }
    this.generatedAt = generated;

    // generated_at is query start, not publication completion. Leave a small
    // settling margin and retry briefly if queries/cache delay the new snapshot.
    const delay = generated + this.interval + 0.5 - now;
    if (delay > 0) {
      return delay * 1000;
    }
    if (this.retries++ < 5) {
      return 1000;
    }
    return this.failureDelay();
  }

  failureDelay() {
    return this.defaultInterval * 1000;
  }
}
