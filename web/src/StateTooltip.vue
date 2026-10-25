<script setup lang="ts">
import { onMounted, onUnmounted, ref, useId } from "vue";

defineOptions({ inheritAttrs: false });
withDefaults(defineProps<{ description: string; focusable?: boolean }>(), {
  focusable: true,
});
const id = useId();
const open = ref(false);
const left = ref(0);
const top = ref(0);
let anchor: HTMLElement | null = null;
function hide() {
  open.value = false;
}
function show(event: Event) {
  anchor = event.currentTarget as HTMLElement;
  position();
  open.value = true;
}
function position() {
  if (anchor === null) return;
  const rect = anchor.getBoundingClientRect();
  left.value = Math.max(8, Math.min(rect.left, window.innerWidth - 288));
  top.value =
    rect.bottom + 150 > window.innerHeight
      ? Math.max(8, rect.top - 130)
      : rect.bottom + 8;
  if (rect.bottom < 0 || rect.top > window.innerHeight) hide();
}
// Follow the anchor when keyboard focus scrolls it into view.
onMounted(() => {
  window.addEventListener("scroll", position, true);
  window.addEventListener("resize", hide);
});
onUnmounted(() => {
  window.removeEventListener("scroll", position, true);
  window.removeEventListener("resize", hide);
});
</script>

<template>
  <span
    v-bind="$attrs"
    class="state-help"
    :tabindex="focusable === false ? undefined : 0"
    :aria-describedby="open ? id : undefined"
    @mouseenter="show"
    @mouseleave="hide"
    @focusin="show"
    @focusout="hide"
    @keydown.esc="hide"
  >
    <slot />
  </span>
  <Teleport to="body">
    <Transition name="tooltip-fade">
      <span
        v-if="open"
        :id="id"
        role="tooltip"
        class="state-tooltip"
        :style="{ left: `${left}px`, top: `${top}px` }"
      >
        {{ description }}
      </span>
    </Transition>
  </Teleport>
</template>
