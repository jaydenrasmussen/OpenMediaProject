import { expect, test } from "@playwright/test";

const BASE = process.env.OMP_URL ?? "http://localhost:8080";
const USER = process.env.OMP_E2E_USER ?? "e2e";
const PASSWORD = process.env.OMP_E2E_PASSWORD ?? "e2e-password-1";

test.use({ baseURL: BASE });

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  // First run asks to create an account, later runs ask to sign in.
  const field = page.getByLabel(/username/i);
  if (await field.isVisible({ timeout: 5000 }).catch(() => false)) {
    await field.fill(USER);
    await page.getByLabel(/^password/i).first().fill(PASSWORD);
    const again = page.getByLabel(/repeat|confirm/i);
    if (await again.isVisible().catch(() => false)) await again.fill(PASSWORD);
    await page.getByRole("button", { name: /create|sign in|log in/i }).click();
  }
  await expect(page.getByRole("link", { name: "Movies", exact: true })).toBeVisible();
});

for (const [name, path] of [
  ["discover", "/discover/"],
  ["movies", "/"],
  ["series", "/series/"],
  ["collections", "/collections/"],
  ["calendar", "/calendar/"],
  ["wanted", "/wanted/"],
  ["activity", "/activity/"],
  ["system", "/system/"],
]) {
  test(`${name} page renders`, async ({ page }) => {
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(e.message));
    await page.goto(path);
    await expect(page.locator("h1")).toBeVisible();
    expect(errors).toEqual([]);
  });
}

test("settings tabs open", async ({ page }) => {
  await page.goto("/settings/");
  for (const tab of ["Quality profiles", "Indexers", "Download clients", "Media servers", "Notifications", "Plugins", "VPN", "UI", "General"]) {
    await page.getByRole("tab", { name: tab, exact: true }).click();
    await expect(page.getByRole("tab", { name: tab, exact: true })).toHaveClass(/active/);
  }
});

test("indexers can be found by filtering", async ({ page }) => {
  await page.goto("/settings/");
  await page.getByRole("tab", { name: "Indexers", exact: true }).click();
  await page.getByRole("button", { name: "Add indexer" }).click();
  await expect(page.getByPlaceholder(/Search \d+ torrent sites/)).toBeVisible();
  await page.getByRole("button", { name: "Public", exact: true }).click();
  await page.getByPlaceholder(/Search \d+ torrent sites/).fill("pirate bay");
  await expect(page.getByText("The Pirate Bay", { exact: true }).first()).toBeVisible();
  await expect(page.getByText(/^1 of \d+ sites/)).toBeVisible();
  await page.getByRole("tab", { name: "Usenet indexers" }).click();
  await page.getByPlaceholder(/Search \d+ usenet indexers/).fill("geek");
  await expect(page.getByText("NZBgeek", { exact: true })).toBeVisible();
});

test("the dark theme can be chosen", async ({ page }) => {
  await page.goto("/settings/");
  await page.getByRole("tab", { name: "UI", exact: true }).click();
  await expect(page.getByRole("button", { name: "Save" })).toBeEnabled(); // the saved values have arrived
  await page.getByLabel("Theme").selectOption("dark");
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await page.getByLabel("Theme").selectOption("auto");
  await page.getByRole("button", { name: "Save" }).click();
});

test("the movie and series editors open", async ({ page }) => {
  for (const path of ["/", "/series/"]) {
    await page.goto(path);
    await page.getByRole("button", { name: "Edit", exact: true }).click();
    await expect(page.getByText("0 selected")).toBeVisible();
    await expect(page.getByRole("button", { name: /^Remove 0$/ })).toBeDisabled();
    await page.getByRole("button", { name: "Done", exact: true }).click();
  }
});

test("movies can be selected, changed together, and removed in the browser", async ({ page, request }) => {
  // Needs a library: a root folder and two movies from the metadata server.
  const root = process.env.OMP_E2E_ROOT ?? "/tmp"; // any folder that exists
  await page.goto("/");
  const api = (path: string, init?: object) => page.request.fetch(`/api/v1${path}`, init);
  const made = await api("/rootfolder", { method: "POST", data: { path: root } });
  test.skip(!made.ok() && made.status() !== 409, `root folder: ${made.status()}`);
  const profiles = await (await api("/qualityprofile")).json();
  const ids: number[] = [];
  for (const tmdb of [603, 604]) {
    const r = await api("/movie", { method: "POST", data: { tmdb_id: tmdb, quality_profile_id: profiles[0].id, root_folder_path: root } });
    test.skip(!r.ok(), "the metadata server could not be reached");
    ids.push((await r.json()).id);
  }

  await page.goto("/");
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("button", { name: "Select all shown" }).click();
  await expect(page.getByText("2 selected")).toBeVisible();
  await page.getByLabel("Monitored").selectOption("no");
  await page.getByRole("button", { name: "Apply to 2" }).click();
  await expect(page.getByText("Updated 2 movies.")).toBeVisible();
  for (const id of ids) {
    expect((await (await api(`/movie/${id}`)).json()).monitored).toBe(false);
  }

  // Remove them (the browser asks twice: remove, and delete files).
  page.on("dialog", (d) => (d.message().includes("delete their folders") ? d.dismiss() : d.accept()));
  await page.getByRole("button", { name: "Select all shown" }).click();
  await page.getByRole("button", { name: "Remove 2" }).click();
  await expect(page.getByText("Removed 2 movies.")).toBeVisible();
  expect(await (await api("/movie")).json()).toEqual([]);
});

test("series can be selected, changed together, and removed in the browser", async ({ page }) => {
  // Series need TMDB: either a real token on the server or e2e/fake-tmdb.mjs.
  const root = process.env.OMP_E2E_ROOT ?? "/tmp";
  await page.goto("/");
  const api = (path: string, init?: object) => page.request.fetch(`/api/v1${path}`, init);
  await api("/rootfolder", { method: "POST", data: { path: root } });
  const profiles = await (await api("/qualityprofile")).json();
  const added = await api("/series", { method: "POST", data: { tmdb_id: 1396, quality_profile_id: profiles[0].id, root_folder_path: root } });
  test.skip(!added.ok(), `series could not be added (${added.status()}): the server may have no TMDB token`);
  const id = (await added.json()).id;

  await page.goto("/series/");
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("button", { name: "Select all shown" }).click();
  await page.getByLabel("Monitored").selectOption("no");
  await page.getByRole("button", { name: "Apply to 1" }).click();
  await expect(page.getByText("Updated 1 series.")).toBeVisible();
  expect((await (await api(`/series/${id}`)).json()).monitored).toBe(false);

  page.on("dialog", (d) => (d.message().includes("delete their folders") ? d.dismiss() : d.accept()));
  await page.getByRole("button", { name: "Select all shown" }).click();
  await page.getByRole("button", { name: "Remove 1" }).click();
  await expect(page.getByText("Removed 1 series.")).toBeVisible();
  expect(await (await api("/series")).json()).toEqual([]);
});
