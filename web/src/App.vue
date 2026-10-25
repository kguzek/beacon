<script setup lang="ts">
import { computed, markRaw, onMounted, onUnmounted, ref } from "vue";
import * as z from "zod";
import {
  currentSchema,
  effectiveStatus,
  endpoint,
  fetchJson,
  historySchema,
  settingsSchema,
  visibleInventory,
  healthSummary,
  healthStates,
  filterServices,
  serviceGroups,
} from "./api";
import type { Observer, Observation } from "./api";
import HistoryBar from "./HistoryBar.vue";
import StateTooltip from "./StateTooltip.vue";
import { useServicePlaceholder } from "./use-service-placeholder";
import { useLocalStorage } from "./use-local-storage";
import { SnapshotPollSchedule } from "./polling";
import {
  combinedDescriptions,
  groupDescriptions,
  monitorDescriptions,
  overviewDescriptions,
  serviceDescriptions,
} from "./status";

const title = ref("Service status");
const observers = ref<Observer[]>([]);
const configError = ref(false);
const now = ref(Date.now() / 1000);
const period = ref<"hour" | "day" | "month">("hour");
const periods = [
  { id: "hour", label: "1 hour" },
  { id: "day", label: "24 hours" },
  { id: "month", label: "30 days" },
] as const;
const inventory = computed(() => visibleInventory(observers.value));
const summary = computed(() =>
  healthSummary(inventory.value, observers.value, now.value),
);
const query = ref("");
const reducedMotion = ref(false);
const serviceNames = computed(() =>
  inventory.value.map((service) => service.name),
);
const animatePlaceholder = computed(
  () => !reducedMotion.value && query.value.length === 0,
);
const { placeholder: searchPlaceholder, phase: placeholderPhase } =
  useServicePlaceholder(serviceNames, animatePlaceholder);
let motionPreference: MediaQueryList | undefined;
function updateMotionPreference() {
  reducedMotion.value = motionPreference?.matches ?? false;
}
const searchInput = ref<HTMLInputElement | null>(null);
const searching = computed(() => query.value.trim().length > 0);
const filtered = computed(() => filterServices(inventory.value, query.value));
const organization = computed(() => {
  const result = serviceGroups(inventory.value, query.value);
  return {
    ...result,
    groups: result.groups.map((group) => ({
      ...group,
      summary: healthSummary(group.services, observers.value, now.value),
    })),
  };
});
const collapsed = useLocalStorage(
  "beacon:collapsed-groups",
  () => [] as string[],
  z.array(z.string()),
);
function expanded(id: string) {
  return searching.value || !collapsed.value.includes(id);
}
function toggleGroup(id: string) {
  if (searching.value) return;
  collapsed.value = collapsed.value.includes(id)
    ? collapsed.value.filter((groupId) => groupId !== id)
    : [...collapsed.value, id];
}
function searchShortcut(event: KeyboardEvent) {
  if (
    (event.ctrlKey || event.metaKey) &&
    !event.altKey &&
    !event.shiftKey &&
    event.key.toLowerCase() === "k" &&
    searchInput.value
  ) {
    event.preventDefault();
    searchInput.value.focus();
    searchInput.value.select();
  }
}
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
async function refresh(
  observer: Observer,
  kind: "current" | "hour" | "day" | "month",
  polling = new SnapshotPollSchedule(
    kind === "current" || kind === "hour" ? 30 : kind === "day" ? 300 : 1800,
  ),
) {
  let delay = polling.failureDelay();
  try {
    if (kind === "current") {
      const value = await fetchJson(
        endpoint(observer.config.url, "/v1/current"),
        currentSchema,
      );
      if (value.location.id !== observer.config.id)
        throw new Error("Observer identity mismatch");
      observer.current = markRaw(value);
      observer.reachable = true;
      delay = polling.nextDelay(value, Date.now() / 1000);
    } else {
      const value = await fetchJson(
        endpoint(observer.config.url, `/v1/history/${kind}`),
        historySchema,
      );
      if (value.location.id !== observer.config.id)
        throw new Error("Observer identity mismatch");
      observer[kind] = markRaw(value);
      delay = polling.nextDelay(value, Date.now() / 1000);
    }
  } catch {
    if (kind === "current") observer.reachable = false;
    else observer[kind] = null;
  } finally {
    if (kind === "current") observer.loading = false;
    schedule(() => {
      void refresh(observer, kind, polling);
    }, delay);
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
  motionPreference = window.matchMedia("(prefers-reduced-motion: reduce)");
  updateMotionPreference();
  motionPreference.addEventListener("change", updateMotionPreference);
  document.addEventListener("keydown", searchShortcut);
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
      hour: null,
      day: null,
      month: null,
    }));
    for (const observer of observers.value)
      for (const kind of ["current", "hour", "day", "month"] as const)
        void refresh(observer, kind);
  } catch {
    configError.value = true;
  }
});
onUnmounted(() => {
  motionPreference?.removeEventListener("change", updateMotionPreference);
  document.removeEventListener("keydown", searchShortcut);
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
      <section
        v-else
        class="notice overview"
        :class="{
          danger: !loading && summary.down > 0,
          neutral: loading || (summary.unknown > 0 && summary.down === 0),
        }"
        aria-label="Service health overview"
      >
        <span class="signal">◉</span>
        <div>
          <h2>
            <StateTooltip
              :description="
                loading
                  ? (overviewDescriptions['Checking service status…'] ?? '')
                  : `${summary.description} ${overviewDescriptions[summary.headline] ?? ''}`.trim()
              "
            >
              {{ loading ? "Checking service status…" : summary.headline }}
            </StateTooltip>
          </h2>
          <p v-if="!loading" class="breakdown">
            <template
              v-for="(state, index) in healthStates.filter(
                (state) => summary[state.id] > 0,
              )"
              :key="state.id"
            >
              <span v-if="index" aria-hidden="true"> · </span>
              <StateTooltip
                :description="
                  `${summary.descriptions[state.id]} ${combinedDescriptions[state.id]}`.trim()
                "
                >{{ summary[state.id] }} {{ state.label }}</StateTooltip
              >
            </template>
          </p>
          <p v-if="!loading && summary.down > 0">
            Availability may vary by location.
          </p>
        </div>
      </section>
      <section
        v-if="!configError && catastrophic"
        class="notice danger monitoring-alert"
        role="alert"
      >
        <h2>Status updates are temporarily unavailable.</h2>
      </section>
      <ul class="server-list" aria-label="Monitoring servers">
        <li v-for="observer in observers" :key="observer.config.id">
          <span
            class="server-icon"
            :class="
              observer.loading
                ? 'unknown'
                : observer.reachable
                  ? 'healthy'
                  : 'down'
            "
            aria-hidden="true"
            >{{ observer.loading ? "◌" : observer.reachable ? "✓" : "×" }}</span
          >
          <span>{{ observer.config.name }}</span>
          <StateTooltip
            class="pill"
            :description="
              observer.loading
                ? monitorDescriptions.connecting
                : observer.reachable
                  ? monitorDescriptions.reachable
                  : monitorDescriptions.unreachable
            "
            :class="
              observer.loading
                ? 'unknown'
                : observer.reachable
                  ? 'healthy'
                  : 'down'
            "
            >{{
              observer.loading
                ? "Connecting"
                : observer.reachable
                  ? "Reachable"
                  : "Unreachable"
            }}</StateTooltip
          >
        </li>
      </ul>
      <div class="section-heading">
        <div>
          <h2>Status by service</h2>
        </div>
        <div class="segmented" aria-label="History window">
          <button
            v-for="option in periods"
            :key="option.id"
            :class="{ selected: period === option.id }"
            :aria-pressed="period === option.id"
            @click="period = option.id"
          >
            {{ option.label }}
          </button>
        </div>
      </div>
      <div class="search-services">
        <label for="service-search">Search services</label>
        <div class="search-controls">
          <div class="search-input">
            <input
              id="service-search"
              ref="searchInput"
              v-model="query"
              :placeholder="searchPlaceholder"
              type="search"
              aria-keyshortcuts="Control+k Meta+k"
              @keydown.esc="query = ''"
            />
            <span v-if="!query" class="search-placeholder" aria-hidden="true">
              <span class="placeholder-text">{{ searchPlaceholder }}</span
              ><span
                class="placeholder-caret"
                :class="{ 'is-deleting': placeholderPhase === 'deleting' }"
              ></span>
            </span>
          </div>
          <kbd>Ctrl/⌘+K</kbd>
          <button
            v-if="query"
            class="clear-search"
            type="button"
            @click="
              query = '';
              searchInput?.focus();
            "
          >
            Clear search
          </button>
        </div>
      </div>
      <p v-if="searching" class="search-count" role="status">
        {{ filtered.length }} of {{ inventory.length }} services
      </p>
      <div
        v-for="(group, index) in organization.groups"
        :key="group.id"
        :class="{ 'service-group': organization.grouped }"
      >
        <div v-if="organization.grouped" class="group-header">
          <h3 class="group-heading">
            <button
              type="button"
              :aria-expanded="expanded(group.id)"
              :aria-controls="`group-${index}`"
              :aria-disabled="searching || undefined"
              @click="toggleGroup(group.id)"
            >
              <span
                ><span
                  class="group-chevron"
                  :class="{ collapsed: !expanded(group.id) }"
                  aria-hidden="true"
                  >▾</span
                >
                {{ group.name }}
                <span class="group-count"
                  >({{
                    searching
                      ? `${group.matches.length} matches / ${group.services.length} services`
                      : group.services.length
                  }})</span
                ></span
              >
            </button>
          </h3>
          <div class="group-summary" aria-label="Group health">
            <StateTooltip
              v-if="group.summary.healthy === group.services.length"
              class="group-state healthy"
              :description="`${group.summary.breakdown}. ${groupDescriptions.healthy}`"
              >All online</StateTooltip
            >
            <template
              v-else
              v-for="state in healthStates.filter(
                (state) => state.id !== 'healthy',
              )"
              :key="state.id"
            >
              <StateTooltip
                v-if="group.summary[state.id] > 0"
                class="group-state"
                :class="state.id"
                :description="`${group.summary.breakdown}. ${group.summary.description} ${groupDescriptions[state.id]}`"
                >{{ group.summary[state.id] }} {{ state.label }}</StateTooltip
              >
            </template>
          </div>
        </div>
        <div :id="`group-${index}`">
          <Transition
            name="group-collapse"
            :css="organization.grouped"
            @before-enter="(element) => element.removeAttribute('inert')"
            @before-leave="(element) => element.setAttribute('inert', '')"
          >
            <div
              v-if="!organization.grouped || expanded(group.id)"
              :class="{ 'group-collapse': organization.grouped }"
            >
              <div :class="{ 'group-collapse-inner': organization.grouped }">
                <div :class="{ 'group-content': organization.grouped }">
                  <section
                    v-for="service in group.matches"
                    :key="service.id"
                    class="service"
                  >
                    <div class="service-header">
                      <div>
                        <h3 class="service-name">
                          {{ service.name }}
                          <StateTooltip
                            v-if="service.requires_vpn"
                            class="vpn-badge"
                            description="VPN required to access this service."
                          >
                            <svg
                              width="14"
                              height="14"
                              viewBox="0 0 24 24"
                              fill="none"
                              stroke="currentColor"
                              stroke-width="1.8"
                              stroke-linecap="round"
                              stroke-linejoin="round"
                              aria-hidden="true"
                            >
                              <path d="M12 3 4 6v6c0 5 8 9 8 9s8-4 8-9V6Z" />
                              <rect x="9" y="11" width="6" height="5" rx="1" />
                              <path d="M10 11V9a2 2 0 0 1 4 0v2" />
                            </svg>
                            VPN
                          </StateTooltip>
                        </h3>
                        <span class="hostname">
                          <a
                            :href="`https://${service.id}`"
                            target="_blank"
                            rel="noopener noreferrer"
                            >{{ service.id }}</a
                          ><span v-if="service.hosting_location">
                            · hosted at {{ service.hosting_location }}</span
                          >
                        </span>
                      </div>
                    </div>
                    <article
                      v-for="observer in observers"
                      :key="observer.config.id"
                      class="observer-row"
                    >
                      <div class="service-top">
                        <div class="service-title">
                          <span
                            class="dot"
                            :class="status(observer, service)"
                          ></span>
                          <div>
                            <h4>{{ observer.config.name }}</h4>
                          </div>
                        </div>
                        <StateTooltip
                          class="state"
                          :class="status(observer, service)"
                          :description="
                            serviceDescriptions[status(observer, service)]
                          "
                          >{{ status(observer, service) }}</StateTooltip
                        >
                      </div>
                      <p
                        v-if="!observer.reachable && !observer.loading"
                        class="observer-warning"
                      >
                        Updates are temporarily unavailable.
                      </p>
                      <p
                        v-else-if="
                          observer.current && now >= observer.current.expires_at
                        "
                        class="observer-warning"
                      >
                        Waiting for a status update.
                      </p>
                      <div class="service-detail">
                        <span
                          >{{
                            lastChecked(
                              observation(observer, service.id)?.observed_at,
                            )
                          }}<template
                            v-if="
                              status(observer, service) !== 'unknown' &&
                              observation(observer, service.id)
                                ?.duration_seconds != null
                            "
                          >
                            ·
                            {{
                              (
                                (observation(observer, service.id)
                                  ?.duration_seconds ?? 0) * 1000
                              ).toFixed(0)
                            }}
                            ms check time</template
                          ></span
                        ><span
                          >Success rate
                          <b>{{
                            percent(
                              history(observer, service.id)
                                ?.observed_success_ratio,
                            )
                          }}</b>
                          · checks received
                          {{
                            percent(history(observer, service.id)?.coverage)
                          }}</span
                        >
                      </div>
                      <HistoryBar
                        :buckets="history(observer, service.id)?.buckets"
                        :bucket-seconds="observer[period]?.bucket_seconds ?? 0"
                        :label="`${service.name} · ${observer.config.name}`"
                      />
                    </article>
                  </section>
                </div>
              </div>
            </div>
          </Transition>
        </div>
      </div>
      <div
        v-if="searching && inventory.length && !filtered.length"
        class="empty"
      >
        <p>No services match ‘{{ query.trim() }}’.</p>
        <button
          class="clear-search"
          type="button"
          @click="
            query = '';
            searchInput?.focus();
          "
        >
          Clear search
        </button>
      </div>
      <p v-if="!inventory.length && !configError" class="empty">
        {{
          loading ? "Checking services…" : "No service information available."
        }}
      </p>
    </main>
    <footer>
      <span class="footer-tagline"
        ><a class="footer-brand" href="https://git.guzek.uk/kguzek/beacon"
          >beacon</a
        >
        <span aria-hidden="true">·</span
        ><span>Public status, private monitoring.</span></span
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
