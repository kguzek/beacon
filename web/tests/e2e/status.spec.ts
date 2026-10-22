import { expect, test } from "@playwright/test";

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
    const bucketSeconds = isMonth ? 21_600 : 900;
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
              window_seconds: isMonth ? 2_592_000 : 86_400,
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
                      last_http_status_code: isMonth ? 204 : 200,
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
  const segments = page
    .locator(".observer-row")
    .first()
    .locator(".history-segment");
  await expect(segments).toHaveCount(3);
  await expect(segments.nth(0)).toHaveClass(/healthy/);
  await expect(segments.nth(1)).toHaveClass(/down/);
  await expect(segments.nth(2)).toHaveClass(/unknown/);
  await segments.first().hover();
  const tooltip = page.getByRole("tooltip");
  await expect(tooltip).toContainText("Example website · primary server");
  await expect(tooltip.locator(".tooltip-date")).toContainText(
    String(new Date().getFullYear()),
  );
  await expect(tooltip).toContainText("Last HTTP status code200");
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
  await page.getByRole("button", { name: "30 days" }).click();
  await segments.first().hover();
  await expect(tooltip).toContainText("Last HTTP status code204");
  await page.setViewportSize({ width: 375, height: 900 });
  await segments.last().hover();
  const box = await tooltip.boundingBox();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(375);
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
    "Online",
  );
  await expect(page.locator(".server-list .down").last()).toHaveText("Offline");
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
