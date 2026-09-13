import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile, readdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import Ajv from "ajv";
import addFormats from "ajv-formats";
import YAML from "yaml";

const packageRoot = new URL("..", import.meta.url);
const lockText = await readFile(new URL("pnpm-lock.yaml", packageRoot), "utf8");
const lock = YAML.parse(lockText);
assert.equal(lock.lockfileVersion, "9.0");
assert.ok(lock.packages && typeof lock.packages === "object");
const lockDigest = createHash("sha256").update(lockText).digest("hex");

function coordinate(value) {
  const split = value.lastIndexOf("@");
  return { name: value.slice(0, split), version: value.slice(split + 1) };
}
function purl(name, version) {
  if (name.startsWith("@")) {
    const [scope, leaf] = name.slice(1).split("/");
    return `pkg:npm/%40${encodeURIComponent(scope)}/${encodeURIComponent(leaf)}@${encodeURIComponent(version)}`;
  }
  return `pkg:npm/${encodeURIComponent(name)}@${encodeURIComponent(version)}`;
}
async function installedPackages() {
  const root = new URL("node_modules/.pnpm/", packageRoot);
  const found = new Map();
  for (const entry of await readdir(root)) {
    const modules = join(root.pathname, entry, "node_modules");
    let names;
    try {
      names = await readdir(modules);
    } catch {
      continue;
    }
    for (const name of names) {
      if (name.startsWith("@")) {
        for (const leaf of await readdir(join(modules, name)))
          await record(join(modules, name, leaf, "package.json"));
      } else await record(join(modules, name, "package.json"));
    }
  }
  async function record(path) {
    const value = JSON.parse(await readFile(path, "utf8"));
    if (value.name && value.version && value.license)
      found.set(`${value.name}@${value.version}`, value);
  }
  return found;
}

const target = new URL("dependency-sbom.cdx.json", packageRoot);
if (process.argv.includes("--write")) {
  const installed = await installedPackages();
  const components = [];
  for (const [key, metadata] of installed) {
    const locked = lock.packages[key];
    if (!locked) continue;
    const { name, version } = coordinate(key);
    const integrity = locked.resolution?.integrity;
    components.push({
      type: "library",
      name,
      version,
      purl: purl(name, version),
      licenses: [{ license: { id: metadata.license } }],
      ...(integrity?.startsWith("sha512-")
        ? {
            hashes: [
              {
                alg: "SHA-512",
                content: Buffer.from(integrity.slice(7), "base64").toString("hex"),
              },
            ],
          }
        : {}),
      externalReferences: [
        { type: "distribution", url: `https://registry.npmjs.org/${name}` },
      ],
    });
  }
  components.sort((a, b) =>
    `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`),
  );
  const sbom = {
    $schema: "http://cyclonedx.org/schema/bom-1.6.schema.json",
    bomFormat: "CycloneDX",
    specVersion: "1.6",
    serialNumber: `urn:uuid:${lockDigest.replace(/^(........)(....)(....)(....)(............).*$/, "$1-$2-$3-$4-$5")}`,
    version: 1,
    metadata: {
      component: {
        type: "application",
        name: "@kyberia/lab-mcp",
        version: "0.1.0",
      },
      properties: [{ name: "kyberia:pnpm-lock-sha256", value: lockDigest }],
    },
    components,
  };
  await writeFile(target, `${JSON.stringify(sbom, null, 2)}\n`);
}

const sbom = JSON.parse(await readFile(target, "utf8"));
assert.equal(
  sbom.metadata.properties.find((value) =>
    value.name === "kyberia:pnpm-lock-sha256",
  )?.value,
  lockDigest,
  "SBOM must match the complete structural pnpm lock",
);
const coordinates = new Set(
  sbom.components.map((value) => `${value.name}@${value.version}`),
);
for (const required of [
  "@modelcontextprotocol/sdk@1.30.0",
  "zod@4.1.11",
  "express@5.2.1",
])
  assert.ok(coordinates.has(required), `missing direct/transitive ${required}`);
for (const component of sbom.components) {
  assert.ok(lock.packages[`${component.name}@${component.version}`]);
  assert.equal(component.purl, purl(component.name, component.version));
  assert.ok(component.licenses?.[0]?.license?.id);
  assert.equal(component.hashes?.length, 1);
  for (const hash of component.hashes)
    assert.match(hash.content, /^[0-9a-f]{128}$/);
}
const schema = JSON.parse(
  await readFile(new URL("cyclonedx-bom-1.6.schema.json", packageRoot), "utf8"),
);
const spdx = JSON.parse(
  await readFile(new URL("spdx.schema.json", packageRoot), "utf8"),
);
const jsf = JSON.parse(
  await readFile(new URL("jsf-0.82.schema.json", packageRoot), "utf8"),
);
const ajv = new Ajv({ strict: false, logger: false });
addFormats(ajv);
ajv.addSchema(spdx).addSchema(jsf);
const validate = ajv.compile(schema);
assert.equal(validate(sbom), true, JSON.stringify(validate.errors));
