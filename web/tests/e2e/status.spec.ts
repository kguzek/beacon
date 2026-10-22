import { expect, test } from "@playwright/test";

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
  await expect(page.locator(".location")).toHaveCount(2);
  await expect(page.locator(".service")).toHaveCount(4);
  await expect(
    page.locator(".service").filter({ hasText: "hidden.org" }),
  ).toHaveCount(0);
  await expect(
    page.locator(".location").first().locator(".state.healthy"),
  ).toHaveCount(1);
  await expect(
    page.locator(".location").first().locator(".state.down"),
  ).toHaveCount(1);
  await expect(
    page.locator(".location").last().locator(".state.unknown"),
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
  await expect(page.locator(".service")).toHaveCount(4);
  await expect(page.getByRole("alert")).toHaveCount(0);
});
