<script setup lang="ts">
import { computed, onUnmounted, ref, shallowRef, useId, watch } from "vue";
import type { History } from "./api";

const stateDescriptions = {
  healthy: "All checks received during this period found the service working.",
  down: "At least one check during this period found a problem.",
  unknown: "Too few checks were received to describe this period reliably.",
};

type Bucket = History["services"][number]["buckets"][number];
const props = defineProps<{
  buckets: Bucket[] | undefined;
  bucketSeconds: number;
  label: string;
}>();
// Yield before replacing a large history so the selector can paint immediately.
// Keep immutable API snapshots out of Vue's deep reactivity.
const displayed = shallowRef(props.buckets);
const displayedBucketSeconds = ref(props.bucketSeconds);
const updating = ref(false);
let updateTimer: ReturnType<typeof setTimeout> | undefined;
let updateFrame = 0;
watch(
  () => [props.buckets, props.bucketSeconds] as const,
  () => {
    clearTimeout(updateTimer);
    cancelAnimationFrame(updateFrame);
    tooltip.value = null;
    updating.value = true;
    updateFrame = requestAnimationFrame(() => {
      updateTimer = setTimeout(() => {
        displayed.value = props.buckets;
        displayedBucketSeconds.value = props.bucketSeconds;
        updating.value = false;
      }, 0);
    });
  },
);
onUnmounted(() => {
  clearTimeout(updateTimer);
  cancelAnimationFrame(updateFrame);
});
const tooltip = ref<{ bucket: Bucket; left: number; top: number } | null>(null);
const tooltipId = useId();
const formatter = new Intl.DateTimeFormat(undefined, {
  year: "numeric",
  month: "numeric",
  day: "numeric",
  hour: "numeric",
  minute: "2-digit",
  second: "2-digit",
  timeZoneName: "short",
});
const segments = computed(() =>
  (displayed.value ?? []).map((bucket) => ({
    bucket,
    state: state(bucket),
    label: `${date(bucket.ends_at)}: ${state(bucket)}, last HTTP status code ${bucket.last_http_status_code ?? "unavailable"}`,
  })),
);
function state(bucket: Bucket) {
  if (bucket.coverage < 0.8 || bucket.observed_success_ratio == null)
    return "unknown";
  return bucket.observed_success_ratio === 1 ? "healthy" : "down";
}
function show(event: Event, bucket: Bucket) {
  const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
  tooltip.value = {
    bucket,
    left: Math.max(
      8,
      Math.min(rect.left + rect.width / 2 - 140, window.innerWidth - 288),
    ),
    top: Math.max(8, rect.top - 210),
  };
}
function date(time: number) {
  return formatter.format(time * 1000);
}
function percent(value: number | null) {
  return value == null ? "—" : `${(value * 100).toFixed(2)}%`;
}
</script>

<template>
  <div
    class="history"
    :aria-label="label"
    :aria-busy="updating"
    @mouseleave="tooltip = null"
  >
    <button
      v-for="segment in segments"
      :key="segment.bucket.ends_at"
      class="history-segment"
      :class="segment.state"
      :aria-label="segment.label"
      :aria-describedby="
        tooltip?.bucket === segment.bucket ? tooltipId : undefined
      "
      @mouseenter="show($event, segment.bucket)"
      @focus="show($event, segment.bucket)"
      @blur="tooltip = null"
      @keydown.esc="tooltip = null"
    ></button>
    <span v-if="!displayed" class="no-history">History unavailable</span>
  </div>
  <Teleport to="body">
    <Transition name="tooltip-fade">
      <div
        v-if="tooltip"
        :id="tooltipId"
        role="tooltip"
        class="history-tooltip"
        :style="{ left: `${tooltip.left}px`, top: `${tooltip.top}px` }"
      >
        <strong>{{ label }}</strong>
        <div class="tooltip-date">
          {{ date(tooltip.bucket.ends_at - displayedBucketSeconds) }}<br />–
          {{ date(tooltip.bucket.ends_at) }}
        </div>
        <dl>
          <div>
            <dt>Check state</dt>
            <dd :class="state(tooltip.bucket)">{{ state(tooltip.bucket) }}</dd>
          </div>
          <div>
            <dt>Last HTTP status code</dt>
            <dd>{{ tooltip.bucket.last_http_status_code ?? "Unavailable" }}</dd>
          </div>
          <div>
            <dt>Success rate</dt>
            <dd>{{ percent(tooltip.bucket.observed_success_ratio) }}</dd>
          </div>
          <div>
            <dt>Checks received</dt>
            <dd>
              {{ tooltip.bucket.samples }} ·
              {{ percent(tooltip.bucket.coverage) }}
            </dd>
          </div>
        </dl>
        <p class="tooltip-explanation">
          {{ stateDescriptions[state(tooltip.bucket)] }}
        </p>
      </div>
    </Transition>
  </Teleport>
</template>
