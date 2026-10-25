import * as z from "zod";

import { describeServices, healthStates } from "./status";

const timeSchema = z.number().nonnegative();
const ratioSchema = z.number().min(0).max(1);
const labelsSchema = z
  .record(
    z
      .string()
      .min(1)
      .max(32)
      .regex(/^[a-z][a-z0-9_]*$/),
    z
      .string()
      .trim()
      .min(1)
      .max(200)
      .regex(/^\P{Cc}*$/u)
      .refine(
        (value) => new TextEncoder().encode(value).length <= 200,
        "Public label values must not exceed 200 UTF-8 bytes",
      ),
  )
  .refine(
    (labels) => Object.keys(labels).length <= 16,
    "At most 16 public labels are supported",
  )
  .meta({ maxProperties: 16 });
const serviceSchema = z.object({
  id: z.string().min(1),
  name: z.string().min(1),
  hidden: z.boolean(),
  hosting_location: z.string().nullable(),
  labels: labelsSchema.optional(),
});
const locationSchema = z.object({ id: z.string(), name: z.string() });
const envelope = {
  schema_version: z.literal(1),
  location: locationSchema,
  generated_at: timeSchema,
  expires_at: timeSchema,
};
export const currentSchema = z
  .object({
    ...envelope,
    services: z
      .array(
        serviceSchema.extend({
          status: z.enum(["healthy", "down", "unknown"]),
          observed_at: timeSchema.nullable(),
          expires_at: timeSchema.nullable(),
          duration_seconds: timeSchema.nullable(),
        }),
      )
      .max(500),
  })
  .superRefine((snapshot, context) => {
    if (
      new Set(snapshot.services.map((s) => s.id)).size !==
      snapshot.services.length
    ) {
      context.addIssue({ code: "custom", message: "Duplicate service IDs" });
    }
  });
export const historySchema = z.object({
  ...envelope,
  window_seconds: z.number().positive(),
  bucket_seconds: z.number().positive(),
  services: z
    .array(
      serviceSchema.extend({
        observed_success_ratio: ratioSchema.nullable(),
        coverage: ratioSchema,
        buckets: z
          .array(
            z.object({
              ends_at: timeSchema,
              observed_success_ratio: ratioSchema.nullable(),
              coverage: ratioSchema,
              samples: z.int().nonnegative(),
              last_http_status_code: z
                .int()
                .min(100)
                .max(599)
                .nullable()
                .optional(),
            }),
          )
          .max(1000),
      }),
    )
    .max(500),
});
export const settingsSchema = z
  .object({
    title: z.string().default("Service status"),
    locations: z
      .array(
        z.object({
          id: z.string().min(1),
          name: z.string().min(1),
          url: z
            .url()
            .refine(
              (value) => ["http:", "https:"].includes(new URL(value).protocol),
              "Only HTTP API URLs are supported",
            ),
        }),
      )
      .min(1)
      .max(20),
  })
  .refine(
    (s) => new Set(s.locations.map((l) => l.id)).size === s.locations.length,
    "Duplicate observer IDs",
  );
export type Current = z.infer<typeof currentSchema>;
export type Observation = Current["services"][number];
export type History = z.infer<typeof historySchema>;
export type LocationConfig = z.infer<
  typeof settingsSchema
>["locations"][number];
export interface Observer {
  config: LocationConfig;
  reachable: boolean;
  loading: boolean;
  current: Current | null;
  hour: History | null;
  day: History | null;
  month: History | null;
}
export function effectiveStatus(
  service: Observation | undefined,
  snapshot: Current | null,
  now: number,
): Observation["status"] {
  if (
    service === undefined ||
    snapshot === null ||
    snapshot.generated_at > now + 5 ||
    now >= snapshot.expires_at ||
    service.observed_at === null ||
    service.observed_at > now + 5 ||
    service.expires_at === null ||
    now >= service.expires_at
  ) {
    return "unknown";
  }
  return service.status;
}
// Union includes in-memory last results, but there is deliberately no durable storage.
export function visibleInventory(observers: Observer[]): Observation[] {
  const inventory = new Map<string, Observation>();
  const hidden = new Set<string>();
  for (const observer of observers) {
    const services = observer.current?.services ?? [];
    for (const item of services) {
      if (item.hidden) {
        hidden.add(item.id);
      }
      const existing = inventory.get(item.id);
      if (existing === undefined) {
        inventory.set(item.id, { ...item });
      } else if (
        existing.labels?.group === undefined &&
        item.labels?.group !== undefined
      ) {
        existing.labels = { ...existing.labels, group: item.labels.group };
      }
    }
  }
  // Conservative visibility across skew: hidden at any observer means hidden on this page.
  return [...inventory.values()]
    .filter((item) => !hidden.has(item.id))
    .sort((a, b) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id));
}
export function combinedStatus(
  id: string,
  observers: Observer[],
  now: number,
): Observation["status"] {
  const states = observers.map((observer) =>
    effectiveStatus(
      observer.current?.services.find((service) => service.id === id),
      observer.current,
      now,
    ),
  );
  if (states.includes("down")) {
    return "down";
  }
  return states.length > 0 && states.every((state) => state === "healthy")
    ? "healthy"
    : "unknown";
}
export function healthSummary(
  services: Observation[],
  observers: Observer[],
  now: number,
) {
  const counts = { healthy: 0, down: 0, unknown: 0 };
  const names: Record<Observation["status"], string[]> = {
    healthy: [],
    down: [],
    unknown: [],
  };
  for (const service of services) {
    const state = combinedStatus(service.id, observers, now);
    counts[state]++;
    names[state].push(service.name);
  }
  const total = services.length;
  const headline =
    total === 0 || counts.unknown === total
      ? "Service status unavailable"
      : counts.healthy === total
        ? "All services online"
        : counts.down === total
          ? "All services down"
          : counts.down > 0
            ? "Partial outage"
            : "Service status incomplete";
  const descriptions = {
    healthy: describeServices(names.healthy, "healthy"),
    down: describeServices(names.down, "down"),
    unknown: describeServices(names.unknown, "unknown"),
  };
  return {
    ...counts,
    headline,
    descriptions,
    description: healthStates
      .map((state) => descriptions[state.id])
      .filter(Boolean)
      .join(" "),
    breakdown: healthStates
      .filter((state) => counts[state.id] > 0)
      .map((state) => `${String(counts[state.id])} ${state.label}`)
      .join(" · "),
  };
}
export function filterServices(services: Observation[], query: string) {
  const normalized = query.trim().toLowerCase();
  return services.filter(
    (service) =>
      service.name.toLowerCase().includes(normalized) ||
      service.id.toLowerCase().includes(normalized),
  );
}
export function serviceGroups(services: Observation[], query: string) {
  const isGrouped = services.some(
    (service) => service.labels?.group !== undefined,
  );
  const groups = new Map<
    string,
    {
      id: string;
      name: string;
      services: Observation[];
      matches: Observation[];
    }
  >();
  const matches = new Set(
    filterServices(services, query).map((service) => service.id),
  );
  for (const service of services) {
    const name = isGrouped ? service.labels?.group : undefined;
    const id = name === undefined ? "fallback" : `named:${name}`;
    let group = groups.get(id);
    if (group === undefined) {
      group = { id, name: name ?? "Other services", services: [], matches: [] };
      groups.set(id, group);
    }
    group.services.push(service);
    if (matches.has(service.id)) {
      group.matches.push(service);
    }
  }
  return {
    grouped: isGrouped,
    groups: [...groups.values()]
      .filter((group) => group.matches.length > 0)
      .sort((a, b) =>
        a.id === "fallback"
          ? 1
          : b.id === "fallback"
            ? -1
            : a.name.localeCompare(b.name) || a.id.localeCompare(b.id),
      ),
  };
}
export async function fetchJson<T>(
  url: string,
  schema: z.ZodType<T>,
): Promise<T> {
  const response = await fetch(url, {
    signal: AbortSignal.timeout(8000),
    cache: "no-cache",
    credentials: "omit",
    referrerPolicy: "no-referrer",
  });
  if (!response.ok) {
    throw new Error("Publisher unavailable");
  }
  const reader = response.body?.getReader();
  if (reader === undefined) {
    throw new Error("Empty response");
  }
  const chunks: Uint8Array[] = [];
  let size = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) {
        break;
      }
      size += value.byteLength;
      if (size > 32 * 1024 * 1024) {
        throw new Error("Response too large");
      }
      chunks.push(value);
    }
  } finally {
    await reader.cancel();
  }
  const bytes = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.length;
  }
  return schema.parse(JSON.parse(new TextDecoder().decode(bytes)));
}
export function endpoint(base: string, path: string): string {
  return `${base.replace(/\/$/, "")}${path}`;
}
