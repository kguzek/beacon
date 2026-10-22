<script setup lang="ts">
import { ref, useId } from "vue";
import type { History } from "./api";

type Bucket = History["services"][number]["buckets"][number];
defineProps<{
  buckets: Bucket[] | undefined;
  bucketSeconds: number;
  label: string;
}>();
const tooltip = ref<{ bucket: Bucket; left: number; top: number } | null>(null);
const tooltipId = useId();
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
  return new Date(time * 1000).toLocaleString(undefined, {
    timeZoneName: "short",
  });
}
function percent(value: number | null) {
  return value == null ? "—" : `${(value * 100).toFixed(2)}%`;
}
</script>

<template>
  <div class="history" :aria-label="label" @mouseleave="tooltip = null">
    <button
      v-for="bucket in buckets ?? []"
      :key="bucket.ends_at"
      class="history-segment"
      :class="state(bucket)"
      :aria-label="`${date(bucket.ends_at)}: ${state(bucket)}, last HTTP status code ${bucket.last_http_status_code ?? 'unavailable'}`"
      :aria-describedby="tooltip?.bucket === bucket ? tooltipId : undefined"
      @mouseenter="show($event, bucket)"
      @focus="show($event, bucket)"
      @blur="tooltip = null"
      @keydown.esc="tooltip = null"
    ></button>
    <span v-if="!buckets" class="no-history">History unavailable</span>
  </div>
  <Teleport to="body">
    <div
      v-if="tooltip"
      :id="tooltipId"
      role="tooltip"
      class="history-tooltip"
      :style="{ left: `${tooltip.left}px`, top: `${tooltip.top}px` }"
    >
      <strong>{{ label }}</strong>
      <div class="tooltip-date">
        {{ date(tooltip.bucket.ends_at - bucketSeconds) }}<br />–
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
    </div>
  </Teleport>
</template>
