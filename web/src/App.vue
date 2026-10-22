<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import {
  currentSchema,
  effectiveStatus,
  endpoint,
  fetchJson,
  historySchema,
  settingsSchema,
  visibleInventory,
} from "./api";
import type { Observer, Observation } from "./api";

const title = ref("Service status");
const observers = ref<Observer[]>([]);
const configError = ref(false);
const now = ref(Date.now() / 1000);
const period = ref<"day" | "month">("day");
const inventory = computed(() => visibleInventory(observers.value));
const catastrophic = computed(
  () =>
    observers.value.length > 0 &&
    observers.value.every((o) => !o.loading && !o.reachable),
);
const loading = computed(
  () => observers.value.length === 0 || observers.value.some((o) => o.loading),
);
let stopped = false;
const timers = new Set<ReturnType<typeof setTimeout>>();
function schedule(callback: () => void, delay: number) {
  if (stopped) return;
  const timer = setTimeout(() => {
    timers.delete(timer);
    callback();
  }, delay);
  timers.add(timer);
}
async function refresh(observer: Observer, kind: "current" | "day" | "month") {
  try {
    if (kind === "current") {
      const value = await fetchJson(
        endpoint(observer.config.url, "/v1/current"),
        currentSchema,
      );
      if (value.location.id !== observer.config.id)
        throw new Error("Observer identity mismatch");
      observer.current = value;
      observer.reachable = true;
    } else {
      const value = await fetchJson(
        endpoint(observer.config.url, `/v1/history/${kind}`),
        historySchema,
      );
      if (value.location.id !== observer.config.id)
        throw new Error("Observer identity mismatch");
      observer[kind] = value;
    }
  } catch {
    if (kind === "current") observer.reachable = false;
    else observer[kind] = null;
  } finally {
    if (kind === "current") observer.loading = false;
    schedule(
      () => {
        void refresh(observer, kind);
      },
      kind === "current" ? 30000 : kind === "day" ? 300000 : 1800000,
    );
  }
}
function observation(observer: Observer, id: string) {
  return observer.current?.services.find((s) => s.id === id);
}
function status(observer: Observer, service: Observation) {
  return effectiveStatus(
    observation(observer, service.id),
    observer.current,
    now.value,
  );
}
function history(observer: Observer, id: string) {
  const snapshot = observer[period.value];
  return snapshot &&
    snapshot.expires_at > now.value &&
    snapshot.generated_at <= now.value + 5
    ? snapshot.services.find((s) => s.id === id)
    : undefined;
}
function percent(value: number | null | undefined) {
  return value == null ? "—" : `${(value * 100).toFixed(2)}%`;
}
function lastChecked(time: number | null | undefined) {
  return time == null
    ? "Not checked yet"
    : `Last checked ${Math.max(0, Math.floor(now.value - time))}s ago`;
}
function tick() {
  now.value = Date.now() / 1000;
  schedule(tick, 1000);
}
onMounted(async () => {
  tick();
  try {
    const settings = await fetchJson(
      `${import.meta.env.BASE_URL}locations.json`,
      settingsSchema,
    );
    title.value = settings.title;
    observers.value = settings.locations.map((config) => ({
      config,
      reachable: false,
      loading: true,
      current: null,
      day: null,
      month: null,
    }));
    for (const observer of observers.value)
      for (const kind of ["current", "day", "month"] as const)
        void refresh(observer, kind);
  } catch {
    configError.value = true;
  }
});
onUnmounted(() => {
  stopped = true;
  for (const timer of timers) clearTimeout(timer);
});
</script>

<template>
  <div class="shell">
    <header>
      <a class="brand" href="https://git.guzek.uk/kguzek/beacon"
        ><span class="beacon">◉</span> beacon<span class="brand-label"
          >SYSTEM STATUS</span
        ></a
      ><span class="live">Live<i aria-hidden="true"></i></span>
    </header>
    <main>
      <div class="eyebrow">SERVICE HEALTH, AT A GLANCE</div>
      <h1>{{ title }}</h1>
      <p class="intro">Current service status across our locations.</p>
      <section v-if="configError" class="notice danger" role="alert">
        <h2>Service status unavailable</h2>
        <p>We couldn’t load this page. Please try again later.</p>
      </section>
      <section v-else-if="catastrophic" class="notice danger" role="alert">
        <h2>Service status unavailable</h2>
        <p>We can’t reach our monitoring locations.</p>
      </section>
      <section v-else class="notice">
        <span class="signal">◉</span>
        <div>
          <h2>
            {{ loading ? "Checking service status…" : "Monitoring is online" }}
          </h2>
        </div>
      </section>
      <div class="section-heading">
        <div>
          <h2>Status by location</h2>
        </div>
        <div class="segmented" aria-label="History window">
          <button
            :class="{ selected: period === 'day' }"
            @click="period = 'day'"
          >
            24 hours</button
          ><button
            :class="{ selected: period === 'month' }"
            @click="period = 'month'"
          >
            30 days
          </button>
        </div>
      </div>
      <section
        v-for="observer in observers"
        :key="observer.config.id"
        class="location"
      >
        <div class="location-header">
          <div>
            <h3>
              <span class="location-icon">⌖</span> {{ observer.config.name }}
            </h3>
            <p>
              {{ inventory.length }} services<span v-if="observer.current">
                · {{ lastChecked(observer.current.generated_at) }}</span
              >
            </p>
          </div>
          <span
            class="pill"
            :class="observer.reachable ? 'healthy' : 'unknown'"
            >{{
              observer.loading
                ? "Connecting"
                : observer.reachable
                  ? "Publisher reachable"
                  : "Observer unavailable"
            }}</span
          >
        </div>
        <div
          v-if="!observer.reachable && !observer.loading"
          class="observer-warning"
        >
          Publisher unreachable: service status unknown.
        </div>
        <div
          v-if="observer.current && now >= observer.current.expires_at"
          class="observer-warning"
        >
          Status out of date. Waiting for an update.
        </div>
        <article v-for="service in inventory" :key="service.id" class="service">
          <div class="service-top">
            <div class="service-title">
              <span class="dot" :class="status(observer, service)"></span>
              <div>
                <h4>{{ service.name }}</h4>
                <span class="hostname"
                  >{{ service.id
                  }}<span v-if="service.hosting_location">
                    · hosted at {{ service.hosting_location }}</span
                  ></span
                >
              </div>
            </div>
            <span class="state" :class="status(observer, service)">{{
              status(observer, service)
            }}</span>
          </div>
          <div class="service-detail">
            <span
              >{{ lastChecked(observation(observer, service.id)?.observed_at)
              }}<template
                v-if="
                  status(observer, service) !== 'unknown' &&
                  observation(observer, service.id)?.duration_seconds != null
                "
              >
                ·
                {{
                  (
                    (observation(observer, service.id)?.duration_seconds ?? 0) *
                    1000
                  ).toFixed(0)
                }}
                ms check time</template
              ></span
            ><span
              >Success rate
              <b>{{
                percent(history(observer, service.id)?.observed_success_ratio)
              }}</b>
              · checks received
              {{ percent(history(observer, service.id)?.coverage) }}</span
            >
          </div>
          <div class="history" aria-label="Check history">
            <span
              v-for="bucket in history(observer, service.id)?.buckets ?? []"
              :key="bucket.ends_at"
              :class="
                bucket.coverage < 0.8 || bucket.observed_success_ratio == null
                  ? 'unknown'
                  : bucket.observed_success_ratio === 1
                    ? 'healthy'
                    : 'down'
              "
              :title="`${new Date(bucket.ends_at * 1000).toISOString()}: success rate ${percent(bucket.observed_success_ratio)}, checks received ${percent(bucket.coverage)}`"
            ></span
            ><span v-if="!history(observer, service.id)" class="no-history"
              >History unavailable</span
            >
          </div>
        </article>
        <p v-if="!inventory.length" class="empty">
          {{
            observer.loading
              ? "Checking services…"
              : "No service information available."
          }}
        </p>
      </section>
    </main>
    <footer>
      <span
        ><a class="footer-brand" href="https://git.guzek.uk/kguzek/beacon"
          >beacon</a
        >
        · Public status, private monitoring.</span
      >
      <span class="copyright"
        >© 2026
        <a
          href="https://www.guzek.uk/?utm_source=beacon&amp;utm_medium=footer&amp;utm_campaign=status_page"
          >Konrad Guzek</a
        ></span
      >
      <span class="footer-note">Auto-refreshes every 30s</span>
    </footer>
  </div>
</template>
