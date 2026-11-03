import * as z from "zod";

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
  requires_vpn: z.boolean().optional(),
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
const httpCodeSchema = z.int().min(100).max(599).nullable();
const historyServiceSchema = serviceSchema.extend({
  observed_success_ratio: ratioSchema.nullable(),
  coverage: ratioSchema,
});
const historyEnvelope = {
  location: locationSchema,
  generated_at: timeSchema,
  expires_at: timeSchema,
  window_seconds: z.number().positive(),
};
/** Hour view: every real Blackbox probe, one entry per Prometheus scrape. */
const probeHistorySchema = z.object({
  schema_version: z.literal(2),
  kind: z.literal("probes"),
  ...historyEnvelope,
  scrape_interval_seconds: z.number().positive(),
  refresh_seconds: z.number().positive(),
  services: z
    .array(
      historyServiceSchema.extend({
        probes: z
          .array(
            z.object({
              at: timeSchema,
              success: z.boolean(),
              http_status_code: httpCodeSchema,
            }),
          )
          .max(3610),
      }),
    )
    .max(500),
});
const segmentSchema = z.object({
  start: timeSchema,
  end: timeSchema,
  state: z.enum(["healthy", "down", "unknown"]),
  samples: z.int().nonnegative(),
  failures: z.int().nonnegative(),
  last_http_status_code: httpCodeSchema,
});
/** Day and month views: contiguous runs of one state. */
const timelineHistorySchema = z.object({
  schema_version: z.literal(2),
  kind: z.literal("timeline"),
  ...historyEnvelope,
  scrape_interval_seconds: z.number().positive(),
  refresh_seconds: z.number().positive(),
  step_seconds: z.number().positive(),
  services: z
    .array(
      historyServiceSchema.extend({
        segments: z.array(segmentSchema).max(10_000),
      }),
    )
    .max(500),
});
/** Older publishers: fixed-width buckets, shown as a timeline. */
const legacyHistorySchema = z.object({
  ...envelope,
  window_seconds: z.number().positive(),
  bucket_seconds: z.number().positive(),
  services: z
    .array(
      historyServiceSchema.extend({
        buckets: z
          .array(
            z.object({
              ends_at: timeSchema,
              observed_success_ratio: ratioSchema.nullable(),
              coverage: ratioSchema,
              samples: z.int().nonnegative(),
              last_http_status_code: httpCodeSchema.optional(),
            }),
          )
          .max(1000),
      }),
    )
    .max(500),
});
export const historySchema = z.union([
  probeHistorySchema,
  timelineHistorySchema,
  legacyHistorySchema,
]);
export type HistoryResponse = z.infer<typeof historySchema>;
export type ProbeHistory = z.infer<typeof probeHistorySchema>;
export type TimelineHistory = z.infer<typeof timelineHistorySchema>;
export type Segment = z.infer<typeof segmentSchema>;
export type Probe = ProbeHistory["services"][number]["probes"][number];
export type History = ProbeHistory | TimelineHistory;

/** Map legacy bucket histories onto the timeline model so one renderer suffices. */
export function normalizeHistory(value: HistoryResponse): History {
  if (value.schema_version === 2) {
    return value;
  }
  const legacy = value;
  return {
    schema_version: 2,
    kind: "timeline",
    location: legacy.location,
    generated_at: legacy.generated_at,
    expires_at: legacy.expires_at,
    window_seconds: legacy.window_seconds,
    scrape_interval_seconds: legacy.bucket_seconds,
    refresh_seconds:
      legacy.window_seconds === 3600
        ? 30
        : legacy.window_seconds === 86_400
          ? 300
          : 1800,
    step_seconds: legacy.bucket_seconds,
    services: legacy.services.map(({ buckets, ...service }) => ({
      ...service,
      segments: mergeSegments(
        buckets.map((bucket) => {
          const state: Segment["state"] =
            bucket.coverage < 0.8 || bucket.observed_success_ratio == null
              ? "unknown"
              : bucket.observed_success_ratio === 1
                ? "healthy"
                : "down";
          return {
            start: bucket.ends_at - legacy.bucket_seconds,
            end: bucket.ends_at,
            state,
            samples: bucket.samples,
            failures: Math.round(
              bucket.samples * (1 - (bucket.observed_success_ratio ?? 1)),
            ),
            last_http_status_code: bucket.last_http_status_code ?? null,
          };
        }),
      ),
    })),
  };
}
/** Merge adjacent segments sharing a state, keeping the latest HTTP code. */
export function mergeSegments(segments: Segment[]): Segment[] {
  const merged: Segment[] = [];
  for (const segment of segments) {
    const last = merged.at(-1);
    if (
      last?.state === segment.state &&
      Math.abs(last.end - segment.start) < 1e-6
    ) {
      last.end = segment.end;
      last.samples += segment.samples;
      last.failures += segment.failures;
      last.last_http_status_code =
        segment.last_http_status_code ?? last.last_http_status_code;
    } else {
      merged.push({ ...segment });
    }
  }
  return merged;
}
/** One slot per expected check: real probes, plus placeholders for missed ones. */
export type ProbeSlot =
  { kind: "probe"; at: number; probe: Probe } | { kind: "missed"; at: number };
export function probeSlots(
  probes: Probe[],
  start: number,
  end: number,
  interval: number,
): ProbeSlot[] {
  const slots: ProbeSlot[] = [];
  // A gap of 1.5 intervals or more means at least one expected check is missing.
  const fill = (from: number, to: number) => {
    const missing = Math.floor((to - from) / interval - 0.5);
    for (let index = 1; index <= missing; index++) {
      slots.push({ kind: "missed", at: from + index * interval });
    }
  };
  let previous = start;
  for (const probe of probes) {
    if (probe.at <= start || probe.at > end) {
      continue;
    }
    fill(previous, probe.at);
    slots.push({ kind: "probe", at: probe.at, probe });
    previous = probe.at;
  }
  fill(previous, end);
  return slots;
}
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
      if (existing !== undefined && item.requires_vpn === true) {
        existing.requires_vpn = true;
      }
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
export const healthStates = [
  { id: "healthy", label: "online" },
  { id: "down", label: "down" },
  { id: "unknown", label: "unknown" },
] as const;

function describeServices(
  names: string[],
  state: (typeof healthStates)[number]["id"],
) {
  if (names.length === 0) {
    return "";
  }
  if (state === "healthy") {
    return "";
  }
  const list = new Intl.ListFormat("en", {
    style: "long",
    type: "conjunction",
  }).format(names);
  const subject = `${names.length === 1 ? "Service" : "Services"} ${list}`;
  const verb = names.length === 1 ? "is" : "are";
  return state === "unknown"
    ? `${subject} ${names.length === 1 ? "has an unknown status" : "have an unknown status"}.`
    : `${subject} ${verb} currently down.`;
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
