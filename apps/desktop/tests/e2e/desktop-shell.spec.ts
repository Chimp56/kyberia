import { test, expect } from "@playwright/test";

const baselineResponse = {
  schema: "kyberia.desktop-ipc/1" as const,
  state: "baseline_only" as const,
  project: {
    projectId: "fixture-project",
    name: "Fixture project",
    state: "baseline_only" as const,
    schemaVersion: "1" as const,
    revision: 0,
    logicalTime: 0,
    hasFloorPlan: false,
    calibrated: false,
  },
  capabilities: [],
};

test("empty shell exposes honest import and keyboard palette states", async ({ page }) => {
  const consoleProblems: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error" || message.type() === "warning") consoleProblems.push(message.text());
  });
  await page.setViewportSize({ width: 1586, height: 960 });
  await page.goto("/");
  await expect(page).toHaveTitle("Kyberia");
  await expect(page.locator("body")).not.toBeEmpty();
  await expect(page.locator("vite-error-overlay, #webpack-dev-server-client-overlay")).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "Import floor plan" })).toBeVisible();
  await expect(page.getByText("Live capture unavailable").first()).toBeVisible();
  await expect(page.getByText("Scale unavailable")).toBeVisible();
  await page.screenshot({ path: "evidence/desktop-shell-1586x960-v3.png", fullPage: true });
  await page.getByRole("button", { name: "Import floor plan" }).click();
  await expect(page.getByRole("heading", { name: "Desktop command required" })).toBeVisible();
  const paletteButton = page.locator(".palette-trigger");
  await paletteButton.click();
  await expect(page.getByRole("dialog", { name: "Command palette" })).toBeVisible();
  const combobox = page.getByRole("combobox", { name: "Search commands" });
  await expect(combobox).toHaveAttribute("aria-controls", "command-list");
  await expect(page.getByRole("listbox", { name: "Commands" })).toBeVisible();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("button", { name: /Select \(V\)/ })).toHaveAttribute("aria-pressed", "true");
  await paletteButton.click();
  await page.keyboard.press("Escape");
  await expect(paletteButton).toBeFocused();
  expect(consoleProblems).toEqual([]);
});

test("loading and error states come from the command result", async ({ page }) => {
  await page.addInitScript(() => {
    window.__RF_ATLAS_IPC__ = {
      currentProject: async () => ({ schema: "kyberia.desktop-ipc/1", state: "no_project", project: null, capabilities: [] }),
      selectOpenProject: async () => ({ schema: "kyberia.desktop-ipc/1", selection: null }),
      openProject: async () => ({ schema: "kyberia.desktop-ipc/1", state: "no_project", project: null, capabilities: [] }),
      createBlankProject: () => new Promise((_, reject) => setTimeout(() => reject({ schema: "kyberia.desktop-ipc/1", code: "corrupt_project", message: "The fixture project is unavailable.", retryable: false }), 200)),
      jobStatus: async ({ jobId }) => ({ schema: "kyberia.desktop-ipc/1", jobId, state: "running", progress: 35 }),
      cancelJob: async ({ jobId }) => ({ schema: "kyberia.desktop-ipc/1", jobId, state: "cancelling" }),
    };
  });
  await page.goto("/");
  await page.getByRole("region", { name: "Floor plan canvas" }).getByRole("button", { name: "New project", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Creating project" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Project unavailable" })).toBeVisible();
  await expect(page.getByText("The fixture project is unavailable.")).toBeVisible();
});

test("native grant open flow is represented without exposing a path", async ({ page }) => {
  await page.addInitScript((response) => {
    window.__RF_ATLAS_IPC__ = {
      currentProject: async () => ({ schema: "kyberia.desktop-ipc/1", state: "no_project", project: null, capabilities: [] }),
      createBlankProject: async () => response,
      selectOpenProject: async () => ({ schema: "kyberia.desktop-ipc/1", selection: { grantId: "opaque-grant", displayName: "Fixture project.rfatlas", kind: "open" } }),
      openProject: async (request) => {
        if (request.grantId !== "opaque-grant") throw new Error("wrong grant");
        return response;
      },
      jobStatus: async ({ jobId }) => ({ schema: "kyberia.desktop-ipc/1", jobId, state: "running", progress: 65 }),
      cancelJob: async ({ jobId }) => ({ schema: "kyberia.desktop-ipc/1", jobId, state: "cancelling" }),
    };
  }, baselineResponse);
  await page.goto("/");
  await page.getByRole("button", { name: "Open project" }).click();
  await expect(page.getByText("Fixture project").first()).toBeVisible();
});

test("mobile inspector toggle keeps the primary actions usable", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 760 });
  await page.goto("/");
  await expect(page.getByRole("button", { name: "Import floor plan" })).toBeVisible();
  await expect(page.getByRole("region", { name: "Floor plan canvas" }).getByRole("button", { name: "New project", exact: true })).toBeVisible();
  const toggle = page.locator(".mobile-inspector-toggle");
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  await expect(page.getByRole("complementary", { name: "Inspector" })).toBeVisible();
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-expanded", "false");
  await page.screenshot({ path: "evidence/mobile-shell-390x760-v3.png", fullPage: true });
});

test("project job exposes progress and user cancellation without swapping session", async ({ page }) => {
  await page.addInitScript(() => {
    let rejectCreate: ((reason: unknown) => void) | null = null;
    let activeJobId = "";
    window.__RF_ATLAS_IPC__ = {
      currentProject: async () => ({ schema: "kyberia.desktop-ipc/1", state: "no_project", project: null, capabilities: [] }),
      selectOpenProject: async () => ({ schema: "kyberia.desktop-ipc/1", selection: null }),
      openProject: async () => ({ schema: "kyberia.desktop-ipc/1", state: "no_project", project: null, capabilities: [] }),
      createBlankProject: ({ jobId }) => {
        activeJobId = jobId;
        return new Promise((_, reject) => { rejectCreate = reject; });
      },
      jobStatus: async ({ jobId }) => ({ schema: "kyberia.desktop-ipc/1", jobId, state: "running", progress: 42 }),
      cancelJob: async ({ jobId }) => {
        if (jobId !== activeJobId) throw new Error("wrong job");
        rejectCreate?.({ schema: "kyberia.desktop-ipc/1", code: "cancelled", message: "The desktop project operation was cancelled.", retryable: true });
        return { schema: "kyberia.desktop-ipc/1", jobId, state: "cancelling" };
      },
    };
  });
  await page.goto("/");
  await page.getByRole("region", { name: "Floor plan canvas" }).getByRole("button", { name: "New project", exact: true }).click();
  await expect(page.getByText("Working — 42%")).toBeVisible();
  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(page.getByRole("heading", { name: "Project unavailable" })).toBeVisible();
  await expect(page.getByText("The desktop project operation was cancelled.")).toBeVisible();
  await expect(page.getByText("Untitled project").first()).toBeVisible();
});

test("retry repeats the failed operation and active-project replacement requires confirmation", async ({ page }) => {
  await page.addInitScript((response) => {
    let creates = 0;
    window.__RF_ATLAS_IPC__ = {
      currentProject: async () => ({ schema: "kyberia.desktop-ipc/1", state: "no_project", project: null, capabilities: [] }),
      selectOpenProject: async () => ({ schema: "kyberia.desktop-ipc/1", selection: { grantId: "opaque-grant", displayName: "Fixture project.rfatlas", kind: "open" } }),
      openProject: async () => response,
      createBlankProject: async () => {
        creates += 1;
        (window as unknown as { __CREATE_COUNT__: number }).__CREATE_COUNT__ = creates;
        if (creates === 1) throw { schema: "kyberia.desktop-ipc/1", code: "storage", message: "Temporary create failure.", retryable: true };
        return response;
      },
      jobStatus: async ({ jobId }) => ({ schema: "kyberia.desktop-ipc/1", jobId, state: "running", progress: 50 }),
      cancelJob: async ({ jobId }) => ({ schema: "kyberia.desktop-ipc/1", jobId, state: "cancelling" }),
    };
  }, baselineResponse);
  await page.goto("/");
  await page.getByRole("button", { name: "Open project" }).click();
  await expect(page.getByText("Fixture project").first()).toBeVisible();

  page.once("dialog", (dialog) => dialog.dismiss());
  await page.getByRole("button", { name: "Create a new project" }).click();
  expect(await page.evaluate(() => (window as unknown as { __CREATE_COUNT__?: number }).__CREATE_COUNT__ ?? 0)).toBe(0);

  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Create a new project" }).click();
  await expect(page.getByText("Temporary create failure.")).toBeVisible();
  await page.getByRole("button", { name: "Try again" }).click();
  await expect(page.getByText("Fixture project").first()).toBeVisible();
  expect(await page.evaluate(() => (window as unknown as { __CREATE_COUNT__?: number }).__CREATE_COUNT__ ?? 0)).toBe(2);
});
