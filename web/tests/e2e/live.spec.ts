import { expect, test } from "@playwright/test";

import { currentSchema, effectiveStatus } from "../../src/api";

test("static OCI frontend renders the live publisher contract", async ({
  page,
}) => {
  test.skip(
    process.env.BEACON_LIVE !== "1",
    "Opt in with BEACON_LIVE=1 and running OCI containers",
  );
  const errors: string[] = [];
  page.on("pageerror", (error) => {
    errors.push(error.message);
  });
  const response = page.waitForResponse(
    (result) => result.url().endsWith("/v1/current") && result.ok(),
  );
  await page.goto("/");
  const currentResponse = await response;
  const snapshot = currentSchema.parse(await currentResponse.json());
  const services = snapshot.services.filter((service) => !service.hidden);
  await expect(page.locator(".service")).toHaveCount(services.length);
  const hostnames = await page.locator(".hostname").allTextContents();
  const ids = hostnames.map((text) => text.split("·", 1)[0]!.trim());
  expect(ids.sort((a, b) => a.localeCompare(b))).toEqual(
    services.map((service) => service.id).sort((a, b) => a.localeCompare(b)),
  );
  for (const status of ["healthy", "down", "unknown"] as const) {
    const now = Date.now() / 1000;
    await expect(page.locator(`.state.${status}`)).toHaveCount(
      services.filter(
        (service) => effectiveStatus(service, snapshot, now) === status,
      ).length,
    );
  }
  expect(errors).toEqual([]);
});
