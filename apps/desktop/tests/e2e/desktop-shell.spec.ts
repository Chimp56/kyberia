import { test, expect } from "@playwright/test";

test("empty shell exposes honest import and command palette states", async ({ page }) => {
  await page.setViewportSize({ width: 1586, height: 960 });
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Import floor plan" })).toBeVisible();
  await expect(page.getByText("Live capture unavailable").first()).toBeVisible();
  await page.screenshot({ path: "evidence/desktop-shell-1586x960.png", fullPage: true });
  await page.getByRole("button", { name: "Import floor plan" }).click();
  await expect(page.getByRole("heading", { name: "Desktop command required" })).toBeVisible();
  await page.keyboard.press("Meta+KeyK");
  await expect(page.getByRole("dialog", { name: "Command palette" })).toBeVisible();
  await expect(page.getByRole("button", { name: "New blank floor" }).last()).toBeVisible();
});

test("loading and error states come from the command result", async ({ page }) => {
  await page.addInitScript(() => {
    window.__RF_ATLAS_IPC__ = {
      currentProject: async () => ({ schema: "kyberia.desktop-ipc/1", state: "no_project", project: null, capabilities: [] }),
      createProject: async () => ({ schema: "kyberia.desktop-ipc/1", state: "no_project", project: null, capabilities: [] }),
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

test("keyboard tools and narrow viewport remain usable", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 760 });
  await page.goto("/");
  await page.keyboard.press("m");
  await expect(page.getByRole("button", { name: /Measure \(M\)/ })).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Control+KeyK");
  await expect(page.getByRole("dialog", { name: "Command palette" })).toBeVisible();
  await expect(page.locator("body")).toHaveCSS("overflow", "hidden");
  await page.screenshot({ path: "evidence/mobile-shell-390x760.png", fullPage: true });
});
