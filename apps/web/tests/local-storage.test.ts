import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { effectScope } from "vue";
import type { EffectScope } from "vue";
import * as z from "zod";

import { useLocalStorage } from "../src/use-local-storage";

const scopes: EffectScope[] = [];
const data = new Map<string, string>();
const storage: Storage = {
  getItem: vi.fn((key: string) => data.get(key) ?? null),
  setItem: vi.fn((key: string, value: string) => {
    data.set(key, value);
  }),
  removeItem: (key) => {
    data.delete(key);
  },
  clear: () => {
    data.clear();
  },
  key: (index) => [...data.keys()][index] ?? null,
  get length() {
    return data.size;
  },
};
let browser: EventTarget;
function preference(key = "groups") {
  const scope = effectScope();
  scopes.push(scope);
  return {
    scope,
    value: scope.run(() =>
      useLocalStorage(key, () => [] as string[], z.array(z.string())),
    )!,
  };
}
function notify(key: string | null = "groups", storageArea: Storage = storage) {
  browser.dispatchEvent(
    Object.assign(new Event("storage"), { key, storageArea }),
  );
}
beforeEach(() => {
  data.clear();
  vi.clearAllMocks();
  browser = Object.assign(new EventTarget(), { localStorage: storage });
  vi.stubGlobal("window", browser);
});
afterEach(() => {
  for (const scope of scopes.splice(0)) scope.stop();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("storage-backed preferences", () => {
  it("restores saved values and persists replacements and nested changes", () => {
    data.set("groups", '["A"]');
    const { value } = preference();
    expect(value.value).toEqual(["A"]);
    expect(storage.setItem).not.toHaveBeenCalled();
    value.value = ["B"];
    expect(data.get("groups")).toBe('["B"]');
    value.value.push("C");
    expect(data.get("groups")).toBe('["B","C"]');
  });

  it("synchronizes hook instances without echo writes and isolates keys", () => {
    const first = preference();
    const second = preference();
    const unrelated = preference("theme");
    first.value.value = ["A"];
    expect(second.value.value).toEqual(["A"]);
    expect(unrelated.value.value).toEqual([]);
    expect(storage.setItem).toHaveBeenCalledTimes(1);
    second.value.value = ["B"];
    expect(first.value.value).toEqual(["B"]);
    expect(storage.setItem).toHaveBeenCalledTimes(2);
  });

  it("receives cross-tab changes, deletion and clear without writing them back", () => {
    const { value } = preference();
    data.set("groups", '["A"]');
    notify("unrelated");
    notify("groups", {} as Storage);
    expect(value.value).toEqual([]);
    notify();
    expect(value.value).toEqual(["A"]);
    data.delete("groups");
    notify();
    expect(value.value).toEqual([]);
    data.set("groups", '["B"]');
    notify();
    data.clear();
    notify(null);
    expect(value.value).toEqual([]);
    expect(storage.setItem).not.toHaveBeenCalled();
  });

  it("ignores malformed JSON and invalid schemas", () => {
    data.set("groups", "not JSON");
    const { value } = preference();
    expect(value.value).toEqual([]);
    value.value = ["A"];
    for (const invalid of ["not JSON", "{}", "[1]"]) {
      data.set("groups", invalid);
      notify();
      expect(value.value).toEqual(["A"]);
    }
  });

  it("keeps state usable when reading or writing storage is blocked", () => {
    vi.mocked(storage.getItem).mockImplementationOnce(() => {
      throw new Error("blocked");
    });
    const { value } = preference();
    vi.mocked(storage.setItem).mockImplementationOnce(() => {
      throw new Error("quota");
    });
    value.value = ["A"];
    expect(value.value).toEqual(["A"]);
    value.value = ["B"];
    expect(data.get("groups")).toBe('["B"]');
  });

  it("handles missing window and disabled localStorage", () => {
    vi.stubGlobal("window", undefined);
    const first = preference();
    first.value.value = ["A"];
    expect(first.value.value).toEqual(["A"]);
    vi.stubGlobal(
      "window",
      Object.defineProperty({}, "localStorage", {
        get() {
          throw new Error("blocked");
        },
      }),
    );
    const second = preference();
    second.value.value.push("B");
    expect(second.value.value).toEqual(["B"]);
  });

  it("removes listeners and stops persisting when its scope is disposed", () => {
    const { scope, value } = preference();
    scope.stop();
    data.set("groups", '["A"]');
    notify();
    expect(value.value).toEqual([]);
    value.value = ["B"];
    expect(data.get("groups")).toBe('["A"]');
  });
});
