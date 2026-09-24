import { defineConfig } from "vitest/config";

export default defineConfig({
  // Keep generated Vite/Vitest cache files inside the assigned worktree.
  cacheDir: "../../.trash/test-runs/desktop-vitest-cache",
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
