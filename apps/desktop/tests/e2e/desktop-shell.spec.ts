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
  await page.setViewportSize({ width: 1586, height: 960 });
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Import floor plan" })).toBeVisible();
  await expect(page.getByText("Live capture unavailable").first()).toBeVisible();
  await expect(page.getByText("Scale unavailable")).toBeVisible();
  await page.screenshot({ path: "evidence/desktop-shell-1586x960-v3.png", fullPage: true });
  await page.getByRole("button", { name: "Import floor plan" }).click();
  await expect(page.getByRole("heading", { name: "Desktop command required" })).toBeVisible();
  const paletteButton = page.locator(".palette-trigger");
  await paletteButton.click();
  await expect(page.getByRole("dialog", { name: "Command palette" })).toBeVisible();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("button", { name: /Select \(V\)/ })).toHaveAttribute("aria-pressed", "true");
  await paletteButton.click();
  await page.keyboard.press("Escape");
  await expect(paletteButton).toBeFocused();
});

test("loading and error states come from the command result", async ({ page }) => {
  await page.addInitScript(() => {
    window.__RF_ATLAS_IPC__ = {
      currentProject: async () => ({ schema: "kyberia.desktop-ipc/1", state: "no_project", project: null, capabilities: [] }),
      selectOpenProject: async () => ({ schema: "kyberia.desktop-ipc/1", selection: null }),
      openProject: async () => ({ schema: "kyberia.desktop-ipc/1", state: "no_project", project: null, capabilities: [] }),
      createBlankProject: () => new Promise((_, reject) => setTimeout(() => reject({ schema: "kyberia.desktop-ipc/1", code: "corrupt_project", message: "The fixture project is unavailable.", retryable: false }), 200)),
    };
  });
  await page.goto("/");
  await page.getByRole("button", { name: "New blank floor" }).click();
  await expect(page.getByRole("heading", { name: "Opening project" })).toBeVisible();
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
  await expect(page.getByRole("button", { name: "New blank floor" })).toBeVisible();
  const toggle = page.locator(".mobile-inspector-toggle");
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  await expect(page.getByRole("complementary", { name: "Inspector" })).toBeVisible();
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-expanded", "false");
  await page.screenshot({ path: "evidence/mobile-shell-390x760-v3.png", fullPage: true });
});
