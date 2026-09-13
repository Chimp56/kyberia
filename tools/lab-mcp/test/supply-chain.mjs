import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";

const lock = await readFile(new URL("../pnpm-lock.yaml", import.meta.url), "utf8");
const packages = lock.split("\npackages:\n")[1]?.split("\nsnapshots:\n")[0];
assert.ok(packages, "pnpm lock package inventory is required");
const components = [];
const lines = packages.split("\n");
for (let index = 0; index < lines.length; index++) {
  const match = /^  ["']?(.+@[^:"']+)["']?:$/.exec(lines[index]);
  if (!match) continue;
  const coordinate = match[1];
  const split = coordinate.lastIndexOf("@");
  const name = coordinate.slice(0, split);
  const version = coordinate.slice(split + 1);
  const block = lines.slice(index + 1, index + 8).join("\n");
  const integrity = /integrity:\s*(sha512-[A-Za-z0-9+/=]+)/.exec(block)?.[1];
  components.push({
    type: "library",
    name,
    version,
    purl: `pkg:npm/${encodeURIComponent(name)}@${version}`,
    ...(integrity ? { hashes: [{ alg: "SHA-512", content: integrity.slice(7) }] } : {}),
    externalReferences: [
      { type: "distribution", url: `https://registry.npmjs.org/${name}` },
    ],
  });
}
components.sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`));
assert.ok(components.length > 50, "transitive inventory is unexpectedly incomplete");
const serialNumber = `urn:uuid:${createHash("sha256")
  .update(lock)
  .digest("hex")
  .replace(/^(........)(....)(....)(....)(............).*$/, "$1-$2-$3-$4-$5")}`;
const sbom = {
  bomFormat: "CycloneDX",
  specVersion: "1.6",
  serialNumber,
  version: 1,
  metadata: { component: { type: "application", name: "@kyberia/lab-mcp", version: "0.1.0" } },
  components,
};
const rendered = `${JSON.stringify(sbom, null, 2)}\n`;
const target = new URL("../dependency-sbom.cdx.json", import.meta.url);
if (process.argv.includes("--write")) await writeFile(target, rendered);
else assert.equal(await readFile(target, "utf8"), rendered, "SBOM must match pnpm lock");
