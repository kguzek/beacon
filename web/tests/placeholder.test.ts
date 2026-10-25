import { afterEach, describe, expect, it, vi } from "vitest";
import { effectScope, nextTick, ref } from "vue";

import {
  serviceCharacters,
  sharedPrefixLength,
  useServicePlaceholder,
} from "../src/use-service-placeholder";

afterEach(() => vi.useRealTimers());

describe("animated service placeholder", () => {
  it("holds names for two seconds and preserves shared prefixes while deleting and typing", () => {
    vi.useFakeTimers();
    const scope = effectScope();
    const { placeholder, phase } = scope.run(() =>
      useServicePlaceholder(ref(["Service A", "Service B"]), ref(true)),
    )!;
    expect(placeholder.value).toBe("Service A");
    expect(phase.value).toBe("holding");
    vi.advanceTimersByTime(2000);
    expect(phase.value).toBe("deleting");
    expect(placeholder.value).toBe("Service A");
    vi.advanceTimersByTime(1000);
    expect(placeholder.value).toBe("Service ");
    expect(phase.value).toBe("typing");
    vi.advanceTimersByTime(1000);
    expect(placeholder.value).toBe("Service B");
    expect(phase.value).toBe("holding");
    vi.advanceTimersByTime(1999);
    expect(placeholder.value).toBe("Service B");
    vi.advanceTimersByTime(2001);
    expect(placeholder.value).toBe("Service A");
    scope.stop();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("cycles all names, deduplicates names, and cancels on inventory or motion changes", async () => {
    vi.useFakeTimers();
    const scope = effectScope();
    const names = ref(["A", "B", "B", "C"]);
    const animated = ref(true);
    const { placeholder } = scope.run(() =>
      useServicePlaceholder(names, animated),
    )!;
    vi.advanceTimersByTime(4000);
    expect(placeholder.value).toBe("B");
    vi.advanceTimersByTime(4000);
    expect(placeholder.value).toBe("C");
    vi.advanceTimersByTime(4000);
    expect(placeholder.value).toBe("A");
    animated.value = false;
    await nextTick();
    expect(vi.getTimerCount()).toBe(0);
    names.value = ["New service"];
    await nextTick();
    expect(placeholder.value).toBe("New service");
    animated.value = true;
    await nextTick();
    expect(vi.getTimerCount()).toBe(0);
    names.value = [];
    await nextTick();
    expect(placeholder.value).toBe("Search services…");
    scope.stop();
  });

  it("compares whole Unicode characters and handles one name being a prefix of another", () => {
    expect(
      sharedPrefixLength(
        serviceCharacters("🟢 App"),
        serviceCharacters("🟢 API"),
      ),
    ).toBe(3);
    expect(serviceCharacters("👩‍💻 é")).toEqual(["👩‍💻", " ", "é"]);
    vi.useFakeTimers();
    const scope = effectScope();
    const { placeholder } = scope.run(() =>
      useServicePlaceholder(ref(["App", "Apple"]), ref(true)),
    )!;
    vi.advanceTimersByTime(2500);
    expect(placeholder.value).toBe("Appl");
    vi.advanceTimersByTime(500);
    expect(placeholder.value).toBe("Apple");
    vi.advanceTimersByTime(3000);
    expect(placeholder.value).toBe("App");
    scope.stop();
  });
});
