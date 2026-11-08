import { onScopeDispose, ref, watch } from "vue";
import type { Ref } from "vue";
import type * as z from "zod";

const localChange = "beacon:local-storage-change";

/** Two-way sync for JSON-serializable preferences, including other tabs and hook instances. */
export function useLocalStorage<T>(
  key: string,
  initialValue: () => T,
  schema: z.ZodType<T>,
): Ref<T> {
  const value = ref(initialValue()) as Ref<T>;
  if (typeof window === "undefined") {
    return value;
  }

  let storage: Storage;
  try {
    storage = window.localStorage;
  } catch {
    // Privacy settings can disable storage entirely; keep an ordinary reactive preference.
    return value;
  }

  let isReceiving = false;
  function receive() {
    try {
      const raw = storage.getItem(key);
      const parsed = schema.safeParse(
        raw === null ? initialValue() : JSON.parse(raw),
      );
      if (
        !parsed.success ||
        JSON.stringify(parsed.data) === JSON.stringify(value.value)
      ) {
        return;
      }
      isReceiving = true;
      value.value = parsed.data;
    } catch {
      // Invalid JSON or inaccessible storage must not break the page.
    } finally {
      isReceiving = false;
    }
  }
  receive();

  const stop = watch(
    value,
    () => {
      if (isReceiving) {
        return;
      }
      try {
        const serialized = JSON.stringify(schema.parse(value.value));
        if (storage.getItem(key) === serialized) {
          return;
        }
        storage.setItem(key, serialized);
        // Native storage events only reach other documents.
        window.dispatchEvent(new Event(localChange));
      } catch {
        // Quota limits and unavailable storage should leave the in-memory state usable.
      }
    },
    { deep: true, flush: "sync" },
  );

  function storageChanged(event: StorageEvent) {
    if (
      event.storageArea === storage &&
      (event.key === key || event.key === null)
    ) {
      receive();
    }
  }
  window.addEventListener("storage", storageChanged);
  window.addEventListener(localChange, receive);
  onScopeDispose(() => {
    stop();
    window.removeEventListener("storage", storageChanged);
    window.removeEventListener(localChange, receive);
  });
  return value;
}
