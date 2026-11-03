<script setup lang="ts">
import {
  computed,
  onUnmounted,
  ref,
  shallowRef,
  useId,
  watch,
  watchEffect,
} from "vue";
import { probeSlots } from "./api";
import type { History, Segment } from "./api";

type State = Segment["state"];
type Item =
  | {
      kind: "probe";
      state: State;
      at: number;
      code: number | null;
      missed: boolean;
    }
  | { kind: "segment"; state: State; segment: Segment };
type ServiceHistory = History["services"][number];

const probeDescriptions: Record<State, string> = {
  healthy: "This check found the service working.",
  down: "This check found a problem.",
  unknown: "No result was received for this expected check.",
};
const segmentDescriptions: Record<State, string> = {
  healthy: "Every check during this period found the service working.",
  down: "At least one check during this period found a problem.",
  unknown: "Too few checks were received to describe this period reliably.",
};

const props = defineProps<{
  snapshot: History | undefined;
  service: ServiceHistory | undefined;
  label: string;
}>();
// Yield before replacing a large history so the selector can paint immediately.
// Keep immutable API snapshots out of Vue's deep reactivity.
const displayed = shallowRef({
  snapshot: props.snapshot,
  service: props.service,
});
const updating = ref(false);
let updateTimer: ReturnType<typeof setTimeout> | undefined;
let updateFrame = 0;
watch(
  () => [props.snapshot, props.service] as const,
  () => {
    clearTimeout(updateTimer);
    cancelAnimationFrame(updateFrame);
    active.value = null;
    ring.value = null;
    updating.value = true;
    updateFrame = requestAnimationFrame(() => {
      updateTimer = setTimeout(() => {
        displayed.value = { snapshot: props.snapshot, service: props.service };
        updating.value = false;
      }, 0);
    });
  },
);
onUnmounted(() => {
  clearTimeout(updateTimer);
  cancelAnimationFrame(updateFrame);
});

const items = computed<Item[]>(() => {
  const { snapshot, service } = displayed.value;
  if (!snapshot || !service) return [];
  if (snapshot.kind === "probes" && "probes" in service) {
    return probeSlots(
      service.probes,
      snapshot.generated_at - snapshot.window_seconds,
      snapshot.generated_at,
      snapshot.scrape_interval_seconds,
    ).map((slot) =>
      slot.kind === "probe"
        ? {
            kind: "probe",
            state: slot.probe.success ? "healthy" : "down",
            at: slot.at,
            code: slot.probe.http_status_code,
            missed: false,
          }
        : {
            kind: "probe",
            state: "unknown",
            at: slot.at,
            code: null,
            missed: true,
          },
    );
  }
  if ("segments" in service)
    return service.segments.map((segment) => ({
      kind: "segment",
      state: segment.state,
      segment,
    }));
  return [];
});
const dense = computed(() => items.value.length > 120);
const isTimeline = computed(
  () => displayed.value.snapshot?.kind === "timeline",
);

const strip = ref<HTMLElement | null>(null);
// The tooltip is anchored by its bottom edge just above the active item.
const active = ref<{ index: number; left: number; anchor: number } | null>(
  null,
);
// The highlight ring keeps its last position while hidden so it can fade out
// in place, then fade in (rather than glide) where the pointer next lands.
const ring = ref<{ left: number; width: number } | null>(null);
const activeItem = computed(() =>
  active.value ? items.value[active.value.index] : undefined,
);
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

function activate(index: number) {
  const element = strip.value?.querySelector(`[data-index="${String(index)}"]`);
  if (!element || !strip.value) return;
  const rect = element.getBoundingClientRect();
  const stripRect = strip.value.getBoundingClientRect();
  // Bars get a ring just outside them; timeline segments get one inside.
  const outset = isTimeline.value ? 0 : 4;
  ring.value = {
    left: rect.left - stripRect.left - outset,
    width: rect.width + 2 * outset,
  };
  active.value = {
    index,
    left: Math.max(
      8,
      Math.min(rect.left + rect.width / 2 - 140, window.innerWidth - 288),
    ),
    anchor: rect.top - 10,
  };
}

// Measure the content so height can transition alongside position: moving
// between items glides and resizes the tooltip instead of snapping.
const content = ref<HTMLElement | null>(null);
const contentHeight = ref<number>();
const gliding = ref(false);
let glideFrame = 0;
watchEffect((onCleanup) => {
  const element = content.value;
  if (!element) return;
  const observer = new ResizeObserver(() => {
    contentHeight.value = element.offsetHeight;
  });
  observer.observe(element);
  onCleanup(() => {
    observer.disconnect();
  });
});
// Appear in place; only animate moves between items while already visible.
watch(
  () => active.value !== null,
  (visible) => {
    cancelAnimationFrame(glideFrame);
    gliding.value = false;
    if (!visible) contentHeight.value = undefined;
    else
      glideFrame = requestAnimationFrame(() => {
        glideFrame = requestAnimationFrame(() => {
          gliding.value = true;
        });
      });
  },
);
onUnmounted(() => {
  cancelAnimationFrame(glideFrame);
});
// Padding (2 × 14px) plus border (2 × 1px) around the measured content;
// the tooltip is border-box, so its height includes them.
const TOOLTIP_CHROME = 30;
const tooltipStyle = computed(() => {
  if (!active.value) return {};
  const outer =
    contentHeight.value === undefined
      ? undefined
      : contentHeight.value + TOOLTIP_CHROME;
  return {
    left: `${String(active.value.left)}px`,
    top: `${String(Math.max(8, active.value.anchor - (outer ?? 210)))}px`,
    height: outer === undefined ? undefined : `${String(outer)}px`,
  };
});
function pointer(event: PointerEvent) {
  const target = (event.target as HTMLElement).closest<HTMLElement>(
    "[data-index]",
  );
  if (target) activate(Number(target.dataset.index));
}
function keydown(event: KeyboardEvent) {
  const last = items.value.length - 1;
  const current = active.value?.index ?? last;
  const next =
    event.key === "ArrowLeft"
      ? current - 1
      : event.key === "ArrowRight"
        ? current + 1
        : event.key === "Home"
          ? 0
          : event.key === "End"
            ? last
            : undefined;
  if (event.key === "Escape") active.value = null;
  if (next === undefined) return;
  event.preventDefault();
  activate(Math.max(0, Math.min(last, next)));
}
function itemLabel(item: Item) {
  return item.kind === "probe"
    ? `${date(item.at)}: ${item.state}`
    : `${date(item.segment.start)} – ${date(item.segment.end)}: ${item.state}`;
}
function date(time: number) {
  return formatter.format(time * 1000);
}
function duration(seconds: number) {
  const units = [
    ["d", 86_400],
    ["h", 3600],
    ["m", 60],
  ] as const;
  const parts: string[] = [];
  let rest = Math.round(seconds);
  for (const [unit, size] of units) {
    if (rest >= size) {
      parts.push(`${String(Math.floor(rest / size))}${unit}`);
      rest %= size;
    }
  }
  if (rest || !parts.length) parts.push(`${String(rest)}s`);
  return parts.slice(0, 2).join(" ");
}
</script>

<template>
  <div
    ref="strip"
    class="history"
    :class="{ dense, timeline: isTimeline }"
    role="group"
    tabindex="0"
    :aria-label="label"
    :aria-busy="updating"
    :aria-describedby="activeItem ? tooltipId : undefined"
    @pointermove="pointer"
    @pointerleave="active = null"
    @focus="activate(items.length - 1)"
    @blur="active = null"
    @keydown="keydown"
  >
    <span
      v-for="(item, index) in items"
      :key="index"
      class="history-segment"
      :class="[item.state, { active: active?.index === index }]"
      :style="
        item.kind === 'segment'
          ? { flexGrow: item.segment.end - item.segment.start }
          : undefined
      "
      :data-index="index"
      :aria-label="itemLabel(item)"
    ></span>
    <div
      v-if="ring"
      class="history-highlight"
      :class="{ gliding, visible: active }"
      :style="{
        left: `${String(ring.left)}px`,
        width: `${String(ring.width)}px`,
      }"
      aria-hidden="true"
    ></div>
    <span v-if="!displayed.service" class="no-history"
      >History unavailable</span
    >
  </div>
  <Teleport to="body">
    <Transition name="tooltip-fade">
      <div
        v-if="active && activeItem"
        :id="tooltipId"
        role="tooltip"
        class="history-tooltip"
        :class="{ gliding }"
        :style="tooltipStyle"
      >
        <div ref="content">
          <strong>{{ label }}</strong>
          <template v-if="activeItem.kind === 'probe'">
            <div class="tooltip-date">{{ date(activeItem.at) }}</div>
            <dl>
              <div>
                <dt>Check state</dt>
                <dd :class="activeItem.state">
                  {{ activeItem.missed ? "missed" : activeItem.state }}
                </dd>
              </div>
              <div>
                <dt>HTTP status code</dt>
                <dd>{{ activeItem.code ?? "Unavailable" }}</dd>
              </div>
            </dl>
            <p class="tooltip-explanation">
              {{ probeDescriptions[activeItem.state] }}
            </p>
          </template>
          <template v-else>
            <div class="tooltip-date">
              {{ date(activeItem.segment.start) }}<br />–
              {{ date(activeItem.segment.end) }}
            </div>
            <dl>
              <div>
                <dt>State</dt>
                <dd :class="activeItem.state">{{ activeItem.state }}</dd>
              </div>
              <div>
                <dt>Duration</dt>
                <dd>
                  {{
                    duration(activeItem.segment.end - activeItem.segment.start)
                  }}
                </dd>
              </div>
              <div>
                <dt>Checks received</dt>
                <dd>{{ activeItem.segment.samples }}</dd>
              </div>
              <div>
                <dt>Failed checks</dt>
                <dd>{{ activeItem.segment.failures }}</dd>
              </div>
              <div v-if="activeItem.state === 'down'">
                <dt>Last HTTP status code</dt>
                <dd>
                  {{
                    activeItem.segment.last_http_status_code ?? "Unavailable"
                  }}
                </dd>
              </div>
            </dl>
            <p class="tooltip-explanation">
              {{ segmentDescriptions[activeItem.state] }}
            </p>
          </template>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
