import { ref, watch } from "vue";
import type { Ref } from "vue";

const segmenter = new Intl.Segmenter(undefined, { granularity: "grapheme" });
export function serviceCharacters(name: string) {
  return [...segmenter.segment(name)].map((part) => part.segment);
}

// Keep the common prefix: only the differing suffix needs to be backspaced.
export function sharedPrefixLength(first: string[], second: string[]) {
  let length = 0;
  while (length < first.length && first[length] === second[length]) {
    length++;
  }
  return length;
}

export function useServicePlaceholder(
  names: Ref<string[]>,
  animated: Ref<boolean>,
) {
  const placeholder = ref("Search services…");
  const phase = ref<"holding" | "deleting" | "typing">("holding");
  watch(
    [() => JSON.stringify(names.value), animated],
    (_, __, onCleanup) => {
      const services = [...new Set(names.value)];
      placeholder.value = services[0] ?? "Search services…";
      phase.value = "holding";
      if (!animated.value || services.length < 2) {
        return;
      }
      let timer: ReturnType<typeof setTimeout>;
      let index = 0;
      onCleanup(() => {
        clearTimeout(timer);
      });

      function morph(from: string[], to: string[], done: () => void) {
        const steps = Math.abs(to.length - from.length);
        if (steps === 0) {
          done();
          return;
        }
        let step = 0;
        function tick() {
          step++;
          const length = from.length + (to.length > from.length ? step : -step);
          placeholder.value = (to.length > from.length ? to : from)
            .slice(0, length)
            .join("");
          if (step === steps) {
            done();
          } else {
            timer = setTimeout(tick, 1000 / steps);
          }
        }
        timer = setTimeout(tick, 1000 / steps);
      }
      function advance() {
        const from = serviceCharacters(services[index] ?? "");
        index = (index + 1) % services.length;
        const to = serviceCharacters(services[index] ?? "");
        const prefix = from.slice(0, sharedPrefixLength(from, to));
        phase.value = "deleting";
        morph(from, prefix, () => {
          phase.value = "typing";
          morph(prefix, to, () => {
            phase.value = "holding";
            timer = setTimeout(advance, 2000);
          });
        });
      }
      timer = setTimeout(advance, 2000);
    },
    { immediate: true },
  );
  return { placeholder, phase };
}
