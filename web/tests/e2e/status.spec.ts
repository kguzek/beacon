import { expect, test } from "@playwright/test";

test("VPN services remain listed with a shield badge and accessible explanation", async ({
  page,
}) => {
  await page.route("**/locations.json", async (route) =>
    route.fulfill({
      json: {
        locations: [
          { id: "primary", name: "Primary", url: "https://primary.test" },
        ],
      },
    }),
  );
  await page.route("https://primary.test/**", async (route) => {
    const now = Date.now() / 1000;
    return route.fulfill({
      json: {
        schema_version: 1,
        location: { id: "primary", name: "Primary" },
        generated_at: now,
        expires_at: now + 3600,
        window_seconds: 3600,
        bucket_seconds: 30,
        services: [true, false, undefined].map((requires_vpn, index) => ({
          id: `service-${String(index)}.test`,
          name: ["Private service", "Public service", "Legacy service"][index],
          hidden: false,
          requires_vpn,
          hosting_location: null,
          status: "healthy",
          observed_at: now,
          expires_at: now + 3600,
          duration_seconds: null,
          observed_success_ratio: null,
          coverage: 0,
          buckets: [],
        })),
      },
    });
  });
  await page.goto("/");
  await expect(page.locator(".service")).toHaveCount(3);
  const badge = page.locator(".vpn-badge");
  await expect(badge).toHaveCount(1);
  await expect(badge.locator("svg")).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Private service VPN" }),
  ).toBeVisible();
  await badge.focus();
  await expect(page.getByRole("tooltip")).toHaveText(
    "VPN required to access this service.",
  );
  await page.keyboard.press("Escape");
  await expect(page.getByRole("tooltip")).toHaveCount(0);
  await page
    .getByRole("searchbox", { name: "Search services" })
    .fill("Private");
  await expect(page.locator(".service")).toHaveCount(1);
  await expect(badge).toBeVisible();
});

test("search placeholder cycles visible names with a backspacing caret and respects reduced motion", async ({
  page,
}) => {
  await page.clock.install();
  await page.clock.pauseAt(new Date());
  await page.route("**/locations.json", async (route) =>
    route.fulfill({
      json: {
        locations: [
          { id: "primary", name: "Primary", url: "https://primary.test" },
        ],
      },
    }),
  );
  await page.route("https://primary.test/**", async (route) => {
    const now = Date.now() / 1000;
    return route.fulfill({
      json: {
        schema_version: 1,
        location: { id: "primary", name: "Primary" },
        generated_at: now,
        expires_at: now + 3600,
        window_seconds: 3600,
        bucket_seconds: 30,
        services: ["Service A", "Service B", "Secret"].map((name, index) => ({
          id: `service-${String(index)}.test`,
          name,
          hidden: name === "Secret",
          hosting_location: null,
          status: "healthy",
          observed_at: now,
          expires_at: now + 3600,
          duration_seconds: null,
          observed_success_ratio: null,
          coverage: 0,
          buckets: [],
        })),
      },
    });
  });
  await page.goto("/");
  const search = page.getByRole("searchbox", { name: "Search services" });
  const caret = page.locator(".placeholder-caret");
  await expect(search).toHaveAttribute("placeholder", "Service A");
  await expect(caret).toHaveCSS("animation-name", "placeholder-blink");
  await page.clock.runFor(2000);
  await expect(caret).toHaveClass(/is-deleting/);
  await expect(caret).toHaveCSS("animation-name", "none");
  await page.clock.runFor(1000);
  await expect(search).toHaveAttribute("placeholder", "Service ");
  await expect(caret).not.toHaveClass(/is-deleting/);
  await page.clock.runFor(1000);
  await expect(search).toHaveAttribute("placeholder", "Service B");
  await page.clock.runFor(4000);
  await expect(search).toHaveAttribute("placeholder", "Service A");
  await page.emulateMedia({ reducedMotion: "reduce" });
  await expect(caret).toHaveCSS("display", "none");
  await page.clock.runFor(10_000);
  await expect(search).toHaveAttribute("placeholder", "Service A");
  await search.fill("my query");
  await expect(page.locator(".search-placeholder")).toHaveCount(0);
  await page.clock.runFor(5000);
  await expect(search).toHaveValue("my query");
  const gaps = await page.locator(".footer-tagline").evaluate((element) => {
    const boxes = [...element.children].map((child) =>
      child.getBoundingClientRect(),
    );
    return [boxes[1]!.left - boxes[0]!.right, boxes[2]!.left - boxes[1]!.right];
  });
  expect(gaps[0]).toBeCloseTo(gaps[1]!, 1);
});

test("monitoring failure does not invalidate retained fresh service health", async ({
  page,
}) => {
  await page.clock.install();
  let isUnreachable = false;
  await page.route("**/locations.json", async (route) =>
    route.fulfill({
      json: {
        locations: [
          { id: "primary", name: "Primary", url: "https://primary.test" },
        ],
      },
    }),
  );
  await page.route("https://primary.test/**", async (route) => {
    if (isUnreachable) {
      return route.abort();
    }
    const now = Date.now() / 1000;
    return route.fulfill({
      json: {
        schema_version: 1,
        location: { id: "primary", name: "Primary" },
        generated_at: now,
        expires_at: now + 90,
        window_seconds: 3600,
        bucket_seconds: 30,
        services: [
          {
            id: "example.org",
            name: "Example",
            hidden: false,
            hosting_location: null,
            status: "healthy",
            observed_at: now,
            expires_at: now + 90,
            duration_seconds: null,
            observed_success_ratio: null,
            coverage: 0,
            buckets: [],
          },
        ],
      },
    });
  });
  await page.goto("/");
  const overview = page.getByRole("region", {
    name: "Service health overview",
  });
  await expect(overview).toContainText("All services online");
  await overview.getByText("1 online", { exact: true }).hover();
  await expect(page.getByRole("tooltip")).toHaveText(
    "Every monitoring location recently found the service working.",
  );
  await page.locator(".intro").hover();
  await expect(page.getByRole("tooltip")).toHaveCount(0);
  await page.locator(".state.healthy").focus();
  await expect(page.getByRole("tooltip")).toHaveText(
    "The latest check from this location found the service working.",
  );
  await page.keyboard.press("Escape");
  await expect(page.getByRole("tooltip")).toHaveCount(0);
  await page.locator(".server-list .pill").hover();
  await expect(page.getByRole("tooltip")).toContainText(
    "We can receive updates from this location.",
  );
  await page.locator(".intro").hover();
  isUnreachable = true;
  await page.clock.fastForward(31_000);
  await expect(page.getByRole("alert")).toHaveText(
    "Status updates are temporarily unavailable.",
  );
  await expect(overview).toContainText("All services online");
  await expect(page.locator(".state.healthy")).toHaveCount(1);
  await page.clock.fastForward(61_000);
  await expect(overview).toContainText("Service status unavailable");
  await expect(overview.locator(".breakdown")).toHaveText("1 unknown");
  await overview.getByText("1 unknown", { exact: true }).hover();
  await expect(page.getByRole("tooltip")).toContainText(
    "Service Example has an unknown status.",
  );
  await page.locator(".intro").hover();
  await expect(page.getByRole("tooltip")).toHaveCount(0);
  await expect(page.locator(".state.unknown")).toHaveCount(1);
  await page.locator(".state.unknown").hover();
  await expect(page.getByRole("tooltip")).toHaveText(
    "There isn’t a recent result from this location, so we can’t confirm the service’s status.",
  );
});

test("initial requests show checking and configuration failures keep their dedicated message", async ({
  page,
}) => {
  let release!: () => void;
  // The project targets ES2022, before Promise.withResolvers.
  // eslint-disable-next-line unicorn/prefer-promise-with-resolvers
  const pending = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/locations.json", async (route) => {
    await pending;
    return route.fulfill({ json: { locations: [] } });
  });
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "Checking service status…" }),
  ).toBeVisible();
  release();
  await expect(page.getByRole("alert")).toContainText(
    "We couldn’t load this page. Please try again later.",
  );
  await expect(
    page.getByRole("heading", { name: "All services online" }),
  ).toHaveCount(0);
});

test("search reveals collapsed groups without changing full health summaries or expansion state", async ({
  page,
}) => {
  let requests = 0;
  await page.route("**/locations.json", async (route) =>
    route.fulfill({
      json: {
        locations: ["primary", "secondary"].map((id) => ({
          id,
          name: id,
          url: `https://${id}.test`,
        })),
      },
    }),
  );
  await page.route("https://*.test/**", async (route) => {
    requests++;
    const url = new URL(route.request().url());
    const id = url.hostname.split(".", 1)[0]!;
    const now = Date.now() / 1000;
    const services = [
      {
        id: "grafana.test",
        name: id === "primary" ? "Grafana" : "Conflicting name",
        labels: id === "primary" ? undefined : { group: "Observability" },
        status: "healthy",
      },
      {
        id: "prometheus.test",
        name: "Prometheus",
        labels: { group: "Observability" },
        status: id === "primary" ? "healthy" : "down",
      },
      { id: "website.test", name: "Website", labels: {}, status: "unknown" },
      {
        id: "other.test",
        name: "Explicit other",
        labels: { group: "Other services" },
        status: "healthy",
      },
      {
        id: "hidden.test",
        name: "Secret Grafana",
        labels: { group: "Private" },
        status: "down",
        hidden: id === "secondary",
      },
    ];
    return route.fulfill({
      json: {
        schema_version: 1,
        location: { id, name: id },
        generated_at: now,
        expires_at: now + 3600,
        ...(!url.pathname.endsWith("current") && {
          window_seconds: 3600,
          bucket_seconds: 30,
        }),
        services: services.map((service) => ({
          hidden: false,
          hosting_location: null,
          ...service,
          observed_at: now,
          expires_at: now + 3600,
          duration_seconds: null,
          observed_success_ratio: null,
          coverage: 0,
          buckets: [],
        })),
      },
    });
  });
  await page.goto("/");
  const overview = page.getByRole("region", {
    name: "Service health overview",
  });
  await expect(overview).toContainText("Partial outage");
  await expect(overview).toContainText("2 online · 1 down · 1 unknown");
  await expect(page.locator(".service")).toHaveCount(4);
  await expect(page.locator(".service-group")).toHaveCount(3);
  await expect(page.locator(".group-heading").first()).toContainText(
    "Observability",
  );
  await expect(page.locator(".group-heading").last()).toContainText(
    "Other services",
  );
  const group = page.getByRole("button", { name: /Observability/ });
  const container = page.locator(".service-group").filter({ has: group });
  await expect(group).toContainText("Observability (2)");
  await group.focus();
  const expandedHeight = (await container.boundingBox())!.height;
  await page.keyboard.press("Enter");
  const collapseHeights = await container.evaluate(async (element) => {
    const heights: number[] = [];
    const start = performance.now();
    while (performance.now() - start < 300) {
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => {
          resolve();
        }),
      );
      heights.push(element.getBoundingClientRect().height);
    }
    return heights;
  });
  expect(
    collapseHeights.some(
      (height) => height > 48 && height < expandedHeight - 1,
    ),
  ).toBe(true);
  await expect(group).toHaveAttribute("aria-expanded", "false");
  await expect(page.locator(".service")).toHaveCount(2);
  await expect(container.locator(".group-summary")).toHaveText("1 down");
  expect((await container.boundingBox())!.height).toBeLessThanOrEqual(48);
  const downBadge = container.getByText("1 down", { exact: true });
  await downBadge.hover();
  await expect(page.getByRole("tooltip")).toContainText(
    "1 online · 1 down. Service Prometheus is currently down.",
  );
  await expect(page.getByRole("tooltip")).toContainText(
    "Availability may vary by location.",
  );
  await expect(group).toHaveAttribute("aria-expanded", "false");
  await page.locator(".intro").hover();
  await expect(page.getByRole("tooltip")).toHaveCount(0);
  await downBadge.focus();
  await expect(page.getByRole("tooltip")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("tooltip")).toHaveCount(0);
  const healthyBadge = page
    .locator(".group-summary")
    .getByText("All online", { exact: true });
  await healthyBadge.hover();
  await expect(page.getByRole("tooltip")).toContainText(
    "1 online. Every service in this group was recently confirmed working from every monitoring location.",
  );
  await page.locator(".intro").hover();
  await expect(page.getByRole("tooltip")).toHaveCount(0);
  const unknownBadge = page
    .locator(".group-summary")
    .getByText("1 unknown", { exact: true });
  await unknownBadge.focus();
  await expect(page.getByRole("tooltip")).toContainText(
    "we can’t confirm their status",
  );
  await expect(page.getByRole("tooltip")).toContainText(
    "Service Website has an unknown status.",
  );
  await page.keyboard.press("Escape");
  await expect(page.getByRole("tooltip")).toHaveCount(0);
  await page.keyboard.press("Control+k");
  const search = page.getByRole("searchbox", { name: "Search services" });
  await expect(search).toBeFocused();
  await search.fill(" GRAFANA ");
  await expect(page.getByRole("status")).toHaveText("1 of 4 services");
  await expect(group).toHaveAttribute("aria-expanded", "true");
  await expect(group).toContainText("1 matches / 2 services");
  await expect(container.locator(".group-summary")).toHaveText("1 down");
  await expect(page.locator(".service")).toHaveCount(1);
  await expect(overview).toContainText("2 online · 1 down · 1 unknown");
  await page.keyboard.press("Meta+k");
  await expect(search).toBeFocused();
  expect(
    await search.evaluate((input: HTMLInputElement) => [
      input.selectionStart,
      input.selectionEnd,
    ]),
  ).toEqual([0, 9]);
  await page.keyboard.press("Escape");
  await expect(search).toHaveValue("");
  await expect(group).toHaveAttribute("aria-expanded", "false");
  await search.fill("website.test");
  await expect(page.locator(".service")).toHaveCount(1);
  await search.fill("Secret");
  await expect(page.getByRole("status")).toHaveText("0 of 4 services");
  await expect(page.getByText("No services match ‘Secret’.")).toBeVisible();
  await page
    .locator(".empty")
    .getByRole("button", { name: "Clear search" })
    .click();
  await expect(search).toHaveValue("");
  await expect(group).toHaveAttribute("aria-expanded", "false");
  await page.getByRole("button", { name: "24 hours" }).click();
  await expect(overview).toContainText("2 online · 1 down · 1 unknown");
  await page.setViewportSize({ width: 375, height: 900 });
  await expect(container.locator(".group-summary")).toBeVisible();
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(375);
  expect((await container.boundingBox())!.height).toBeLessThanOrEqual(90);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await group.click();
  await expect(group).toHaveAttribute("aria-expanded", "true");
  await expect(container.locator(".service")).toHaveCount(2);
  await expect(container.locator(".group-chevron")).toHaveCSS(
    "transition-duration",
    "0s",
  );
  await group.click();
  await expect(container.locator(".service")).toHaveCount(0);
  expect(requests).toBe(8);
  await page.reload();
  await expect(group).toHaveAttribute("aria-expanded", "false");
  await expect(container.locator(".service")).toHaveCount(0);
  const otherTab = await page.context().newPage();
  await otherTab.route("**/*", async (route) =>
    route.fulfill({
      contentType: "text/html",
      body: "<!doctype html><title>Preferences</title>",
    }),
  );
  await otherTab.goto(new URL("/storage-sync", page.url()).href);
  await otherTab.evaluate(() => {
    localStorage.setItem("beacon:collapsed-groups", "[]");
  });
  await expect(group).toHaveAttribute("aria-expanded", "true");
  await group.click();
  await expect
    .poll(async () =>
      otherTab.evaluate(() => localStorage.getItem("beacon:collapsed-groups")),
    )
    .not.toBe("[]");
  await otherTab.evaluate(() => {
    localStorage.removeItem("beacon:collapsed-groups");
  });
  await expect(group).toHaveAttribute("aria-expanded", "true");
  await otherTab.close();
});

test("services group server histories with custom segment tooltips", async ({
  page,
}) => {
  await page.route("**/locations.json", async (route) =>
    route.fulfill({
      json: {
        locations: ["primary", "secondary"].map((id) => ({
          id,
          name: `${id} server`,
          url: `https://${id}.test`,
        })),
      },
    }),
  );
  await page.route("https://*.test/**", async (route) => {
    const url = new URL(route.request().url());
    const id = url.hostname.split(".", 1)[0]!;
    const now = Date.now() / 1000;
    const isMonth = url.pathname.endsWith("month");
    const isHour = url.pathname.endsWith("hour");
    const bucketSeconds = isMonth ? 21_600 : isHour ? 30 : 900;
    const service = {
      id: "example.org",
      name: "Example website",
      hidden: false,
      hosting_location: null,
    };
    return route.fulfill({
      json: {
        schema_version: 1,
        location: { id, name: id },
        generated_at: now,
        expires_at: now + 3600,
        ...(url.pathname.endsWith("current")
          ? {
              services: [
                {
                  ...service,
                  status: "healthy",
                  observed_at: now,
                  expires_at: now + 3600,
                  duration_seconds: 0.1,
                },
              ],
            }
          : {
              window_seconds: isMonth ? 2_592_000 : isHour ? 3600 : 86_400,
              bucket_seconds: bucketSeconds,
              services: [
                {
                  ...service,
                  observed_success_ratio: 0.75,
                  coverage: 0.8,
                  buckets: [
                    {
                      ends_at: now - 2 * bucketSeconds,
                      observed_success_ratio: 1,
                      coverage: 1,
                      samples: 30,
                      last_http_status_code: isMonth ? 204 : isHour ? 201 : 200,
                    },
                    {
                      ends_at: now - bucketSeconds,
                      observed_success_ratio: 0.5,
                      coverage: 1,
                      samples: 30,
                      last_http_status_code: 503,
                    },
                    {
                      ends_at: now,
                      observed_success_ratio: null,
                      coverage: 0,
                      samples: 0,
                    },
                  ],
                },
              ],
            }),
      },
    });
  });
  await page.goto("/");
  await expect(page.locator(".service")).toHaveCount(1);
  await expect(page.locator(".observer-row")).toHaveCount(2);
  const serviceLink = page.getByRole("link", {
    name: "example.org",
    exact: true,
  });
  await expect(serviceLink).toHaveAttribute("href", "https://example.org");
  await expect(serviceLink).toHaveAttribute("target", "_blank");
  await expect(serviceLink).toHaveAttribute("rel", "noopener noreferrer");
  const segments = page
    .locator(".observer-row")
    .first()
    .locator(".history-segment");
  await expect(segments).toHaveCount(3);
  await expect(
    page.getByRole("button", { name: "1 hour", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  await expect(segments.nth(0)).toHaveClass(/healthy/);
  await expect(segments.nth(1)).toHaveClass(/down/);
  await expect(segments.nth(2)).toHaveClass(/unknown/);
  await segments.first().hover();
  const tooltip = page.getByRole("tooltip");
  await expect(tooltip).toContainText("Example website · primary server");
  await expect(tooltip.locator(".tooltip-date")).toContainText(
    String(new Date().getFullYear()),
  );
  await expect(tooltip).toContainText("Last HTTP status code201");
  await expect(tooltip).toContainText("Check statehealthy");
  await segments.nth(1).hover();
  await expect(tooltip).toContainText("Last HTTP status code503");
  await expect(tooltip).toContainText("Check statedown");
  await segments.nth(2).hover();
  await expect(tooltip).toContainText("Last HTTP status codeUnavailable");
  await expect(tooltip).toContainText("Check stateunknown");
  await page.getByRole("heading", { name: "Status by service" }).hover();
  await expect(tooltip).toHaveCount(0);
  await segments.first().focus();
  await expect(tooltip).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(tooltip).toHaveCount(0);
  await page.getByRole("button", { name: "24 hours" }).click();
  await segments.first().hover();
  await expect(tooltip).toContainText("Last HTTP status code200");
  await page.getByRole("heading", { name: "Status by service" }).hover();
  await page.getByRole("button", { name: "30 days" }).click();
  await segments.first().hover();
  await expect(tooltip).toContainText("Last HTTP status code204");
  await page.setViewportSize({ width: 375, height: 900 });
  await segments.last().hover();
  const box = await tooltip.boundingBox();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(375);
});

test("large histories switch from prefetched snapshots without a long pause", async ({
  page,
}) => {
  const requests: string[] = [];
  await page.route("**/locations.json", async (route) =>
    route.fulfill({
      json: {
        locations: [
          { id: "primary", name: "Primary", url: "https://primary.test" },
        ],
      },
    }),
  );
  await page.route("https://primary.test/**", async (route) => {
    const kind = route.request().url().split("/").at(-1)!;
    requests.push(kind);
    const now = Date.now() / 1000;
    const bucketSeconds = kind === "hour" ? 30 : kind === "day" ? 900 : 21_600;
    const count = kind === "day" ? 96 : 120;
    return route.fulfill({
      json: {
        schema_version: 1,
        location: { id: "primary", name: "Primary" },
        generated_at: now,
        expires_at: now + 3600,
        window_seconds: bucketSeconds * count,
        bucket_seconds: bucketSeconds,
        services: Array.from({ length: 40 }, (_service, serviceIndex) => ({
          id: `service-${String(serviceIndex)}.test`,
          name: `Service ${String(serviceIndex)}`,
          hidden: false,
          hosting_location: null,
          ...(kind === "current"
            ? {
                status: "healthy",
                observed_at: now,
                expires_at: now + 3600,
                duration_seconds: 0.1,
              }
            : {
                observed_success_ratio: 1,
                coverage: 1,
                buckets: Array.from(
                  { length: count },
                  (_bucket, bucketIndex) => ({
                    ends_at: now - (count - bucketIndex) * bucketSeconds,
                    observed_success_ratio: 1,
                    coverage: 1,
                    samples: bucketSeconds / 30,
                  }),
                ),
              }),
        })),
      },
    });
  });
  await page.goto("/");
  await expect(page.locator(".history-segment")).toHaveCount(4800);
  await expect.poll(() => requests.length).toBe(4);
  for (const [label, count] of [
    ["24 hours", 3840],
    ["30 days", 4800],
    ["1 hour", 4800],
  ] as const) {
    // Measure through the first painted frame, not just the click handler.
    const elapsed = await page.evaluate(async (targetLabel) => {
      const start = performance.now();
      const button = [...document.querySelectorAll(".segmented button")].find(
        (candidate) => candidate.textContent.trim() === targetLabel,
      ) as HTMLButtonElement;
      button.click();
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            resolve();
          });
        }),
      );
      return performance.now() - start;
    }, label);
    expect(elapsed).toBeLessThan(1000);
    await expect(
      page.getByRole("button", { name: label, exact: true }),
    ).toHaveAttribute("aria-pressed", "true");
    await expect(page.locator(".history-segment")).toHaveCount(count);
    await expect(page.locator('.history[aria-busy="true"]')).toHaveCount(0);
  }
  expect(requests).toHaveLength(4);
});

test("location failures, hidden services, observation expiry and recovery", async ({
  page,
}) => {
  let isOffline = false;
  await page.route("**/locations.json", async (route) =>
    route.fulfill({
      json: {
        title: "Our infrastructure",
        locations: [
          {
            id: "primary",
            name: "Primary server",
            url: "https://primary.test",
          },
          {
            id: "secondary",
            name: "Secondary server",
            url: "https://secondary.test",
          },
        ],
      },
    }),
  );
  await page.route("https://*.test/**", async (route) => {
    if (isOffline || route.request().url().includes("secondary.test")) {
      return route.abort();
    }
    const now = Date.now() / 1000;
    return route.fulfill({
      json: {
        schema_version: 1,
        location: { id: "primary", name: "Primary server" },
        generated_at: now,
        expires_at: now + 60,
        services: [
          {
            id: "example.org",
            name: "Example website",
            hidden: false,
            hosting_location: "primary",
            status: "healthy",
            observed_at: now,
            expires_at: now + 2,
            duration_seconds: 0.1,
          },
          {
            id: "failed.org",
            name: "Failed service",
            hidden: false,
            hosting_location: null,
            status: "down",
            observed_at: now,
            expires_at: now + 2,
            duration_seconds: null,
          },
          {
            id: "hidden.org",
            name: "Secret page service",
            hidden: true,
            hosting_location: null,
            status: "down",
            observed_at: now,
            expires_at: now + 60,
            duration_seconds: null,
          },
        ],
      },
    });
  });
  await page.goto("/");
  // The unavailable location retains rows discovered through the working one.
  await expect(page.locator(".server-list li")).toHaveCount(2);
  await expect(page.locator(".server-list .healthy").last()).toHaveText(
    "Reachable",
  );
  await expect(page.locator(".server-list .down").last()).toHaveText(
    "Unreachable",
  );
  await expect(page.locator(".service")).toHaveCount(2);
  await expect(page.locator(".observer-row")).toHaveCount(4);
  await expect(
    page.getByRole("heading", { name: "Example website", exact: true }),
  ).toHaveCount(1);
  await expect(
    page.locator(".service").filter({ hasText: "hidden.org" }),
  ).toHaveCount(0);
  await expect(
    page
      .locator(".observer-row")
      .filter({ hasText: "Primary server" })
      .locator(".state.healthy"),
  ).toHaveCount(1);
  await expect(
    page
      .locator(".observer-row")
      .filter({ hasText: "Primary server" })
      .locator(".state.down"),
  ).toHaveCount(1);
  await expect(
    page
      .locator(".observer-row")
      .filter({ hasText: "Secondary server" })
      .locator(".state.unknown"),
  ).toHaveCount(2);
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.locator(".state.healthy")).toHaveCount(1);

  // Both successful and failed observations expire without another fetch.
  await expect(page.locator(".state.unknown")).toHaveCount(4, {
    timeout: 5000,
  });
  await expect(page.locator(".state.healthy, .state.down")).toHaveCount(0);

  // With no reachable inventory after reload, do not invent service failures.
  isOffline = true;
  await page.reload();
  await expect(page.getByRole("alert")).toHaveCount(1);
  await expect(page.locator(".state.down")).toHaveCount(0);
  await expect(page.locator(".service")).toHaveCount(0);

  isOffline = false;
  await page.reload();
  await expect(page.locator(".state.healthy")).toHaveCount(1);
  await expect(page.locator(".state.down")).toHaveCount(1);
  await expect(page.locator(".service")).toHaveCount(2);
  await expect(page.locator(".observer-row")).toHaveCount(4);
  await expect(page.getByRole("alert")).toHaveCount(0);
});
