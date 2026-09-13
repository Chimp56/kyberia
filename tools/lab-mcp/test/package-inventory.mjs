import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";

const output = execFileSync("npm", ["pack", "--dry-run", "--json"], {
  cwd: new URL("..", import.meta.url),
  encoding: "utf8",
  env: {
    ...process.env,
    npm_config_cache: new URL("../.trash/test-runs/npm-cache/", import.meta.url)
      .pathname,
  },
});
const inventory = JSON.parse(output)[0].files.map((entry) => entry.path);
for (const forbidden of ["node_modules/", ".tools/", ".trash/", "test/"])
  assert.equal(
    inventory.some((path) => path.startsWith(forbidden)),
    false,
    `package contains ${forbidden}`,
  );
assert.ok(inventory.includes("dist/src/main.js"));
assert.ok(inventory.includes("dist/runner-bundle.mjs"));
for (const required of [
  "README.md",
  "config.example.json",
  "config.remote.example.json",
  "runner.example.json",
  "CYCLONEDX-SCHEMA-LICENSE",
  "ZOD-LICENSE",
  "dependency-sbom.cdx.json",
])
  assert.ok(inventory.includes(required), `package is missing ${required}`);
assert.ok(
  inventory.every(
    (path) =>
      path === "package.json" ||
      path === "README.md" ||
      path === "CYCLONEDX-SCHEMA-LICENSE" ||
      path === "ZOD-LICENSE" ||
      path === "dependency-sbom.cdx.json" ||
      path === "dist/runner-bundle.mjs" ||
      path.endsWith(".example.json") ||
      path.endsWith(".schema.json") ||
      path.startsWith("dist/src/"),
  ),
);
