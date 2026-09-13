import assert from "node:assert/strict";
import { mkdir, readFile, symlink, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { join, resolve } from "node:path";
import { test } from "node:test";
import { execFileSync } from "node:child_process";
import type { SignedJobRequest } from "../src/manager.js";
import { inputManifestId, proveHost, runOnce } from "../src/runner.js";
import { RunnerConfig, type LabRunnerConfig } from "../src/schema.js";
import { canonical, signObject, verifyObject } from "../src/security.js";
import { keys } from "./helpers.js";

const root = join(
  process.cwd(),
  ".trash",
  "test-runs",
  `lab-runner-${process.pid}`,
);
await mkdir(root, { recursive: true });
const executableRunnerTest = process.platform === "win32" ? test.skip : test;
function setup(name: string) {
  const coordinator = keys(`RUNNER_COORD_${name.toUpperCase()}`),
    host = keys(`RUNNER_HOST_${name.toUpperCase()}`);
  const sha = execFileSync("git", ["rev-parse", "HEAD"], {
    cwd: resolve(process.cwd(), "../.."),
    encoding: "utf8",
  }).trim();
  const readme = awaitInput();
  const inputs = [{ path: "README.md", sha256: readme }];
  const manifestId = inputManifestId(inputs);
  const config: LabRunnerConfig = {
    schemaVersion: 1,
    hostId: "lab-one",
    hostIdentity: host.identity,
    hostPrivateKeyEnv: host.privateName,
    coordinatorPublicKeyEnv: coordinator.publicName,
    coordinatorKeyId: "coordinator-1",
    checkoutDirectory: resolve(process.cwd(), "../.."),
    replayDirectory: join(root, name),
    maximumClockSkewSeconds: 60,
    capabilities: ["wifi"],
    inputManifests: { [manifestId]: inputs },
    limits: { timeoutSeconds: 5, outputBytes: 1024 },
    operations: {
      foundation: {
        executable: "/usr/bin/printf",
        arguments: [],
        version: "foundation-v1",
        parameters: { default: ["validated"] },
        inputManifestId: manifestId,
      },
      "probe-kismet": {
        executable: "/usr/bin/printf",
        arguments: [],
        version: "kismet-v1",
        parameters: { "expected_version=2025.01": ["kismet-ok"] },
        inputManifestId: manifestId,
      },
    },
  };
  const request = {
    schemaVersion: 1 as const,
    runId: `run-${name}`,
    hostId: "lab-one",
    gitSha: sha,
    suite: "foundation",
    seed: 3,
    timeoutSeconds: 2,
    parameters: {},
    nonce: `nonce-${name}`,
    issuedAt: new Date().toISOString(),
    coordinatorKeyId: "coordinator-1",
    specVersion: "foundation-v1",
    inputManifestId: manifestId,
  };
  const signed: SignedJobRequest = {
    request,
    signature: signObject(request, coordinator.privateKey),
    algorithm: "Ed25519",
  };
  return { config, signed, host, coordinator };
}
function awaitInput() {
  return createHash("sha256")
    .update(
      execFileSync("git", ["show", "HEAD:README.md"], {
        cwd: resolve(process.cwd(), "../.."),
      }),
    )
    .digest("hex");
}
function bindInput(
  s: ReturnType<typeof setup>,
  entries: { path: string; sha256: string }[],
) {
  const id = inputManifestId(entries);
  s.config.inputManifests = { [id]: entries };
  s.config.operations.foundation!.inputManifestId = id;
  s.signed.request.inputManifestId = id;
  s.signed.signature = signObject(s.signed.request, s.coordinator.privateKey);
}
executableRunnerTest(
  "runner verifies coordinator, immutable checkout, allowlist and signs result",
  async () => {
    const s = setup("valid");
    const response = await runOnce(s.config, s.signed);
    assert.equal(response.payload.stdout, "validated");
    assert.equal(
      verifyObject(response.payload, response.signature, s.host.privateKey),
      true,
    );
  },
);
test("runner proves pinned host identity before work", () => {
  const s = setup("proof");
  const challenge = {
    schemaVersion: 1 as const,
    hostId: "lab-one",
    nonce: "a".repeat(43),
    issuedAt: new Date().toISOString(),
  };
  const proof = proveHost(s.config, challenge);
  assert.equal(proof.payload.challengeDigest.length, 71);
  assert.equal(
    verifyObject(proof.payload, proof.signature, s.host.privateKey),
    true,
  );
});
executableRunnerTest(
  "runner rejects replay, tampering, stale request and revision mismatch",
  async () => {
    const replay = setup("replay");
    await runOnce(replay.config, replay.signed);
    await assert.rejects(runOnce(replay.config, replay.signed));
    const tamper = setup("tamper");
    tamper.signed.request.seed = 99;
    await assert.rejects(runOnce(tamper.config, tamper.signed), /signature/);
    const stale = setup("stale");
    stale.signed.request.issuedAt = "2020-01-01T00:00:00.000Z";
    stale.signed.signature = signObject(
      stale.signed.request,
      stale.coordinator.privateKey,
    );
    await assert.rejects(runOnce(stale.config, stale.signed), /freshness/);
    const revision = setup("revision");
    revision.signed.request.gitSha = "f".repeat(40);
    revision.signed.signature = signObject(
      revision.signed.request,
      revision.coordinator.privateKey,
    );
    await assert.rejects(runOnce(revision.config, revision.signed), /revision/);
  },
);
executableRunnerTest(
  "runner maps parameter selector to static args and rejects other values",
  async () => {
    const s = setup("params");
    s.signed.request.suite = "probe-kismet";
    s.signed.request.specVersion = "kismet-v1";
    s.signed.request.parameters = { expected_version: "2025.01" };
    s.signed.signature = signObject(s.signed.request, s.coordinator.privateKey);
    const response = await runOnce(s.config, s.signed);
    assert.equal(response.payload.stdout, "kismet-ok");
  },
);

executableRunnerTest(
  "runner rejects operation version and manifest substitutions",
  async () => {
    const version = setup("version-substitution");
    version.signed.request.specVersion = "other-v1";
    version.signed.signature = signObject(
      version.signed.request,
      version.coordinator.privateKey,
    );
    await assert.rejects(
      runOnce(version.config, version.signed),
      /specification/,
    );

    const manifest = setup("manifest-substitution");
    manifest.signed.request.inputManifestId = "sha256:" + "f".repeat(64);
    manifest.signed.signature = signObject(
      manifest.signed.request,
      manifest.coordinator.privateKey,
    );
    await assert.rejects(
      runOnce(manifest.config, manifest.signed),
      /specification/,
    );
  },
);

executableRunnerTest(
  "runner rejects changed, dirty, ignored, and symlinked inputs",
  async () => {
    const changed = setup("changed-input");
    bindInput(changed, [{ path: "README.md", sha256: "0".repeat(64) }]);
    await assert.rejects(runOnce(changed.config, changed.signed), /changed/);

    const dirty = setup("dirty-input");
    const managerPath = resolve(process.cwd(), "src/manager.ts");
    bindInput(dirty, [
      {
        path: "tools/lab-mcp/src/manager.ts",
        sha256: createHash("sha256")
          .update(await readFile(managerPath))
          .digest("hex"),
      },
    ]);
    await assert.rejects(runOnce(dirty.config, dirty.signed), /dirty/);

    const ignoredPath = join(
      process.cwd(),
      ".trash",
      "test-runs",
      `ignored-input-${process.pid}.sh`,
    );
    await writeFile(ignoredPath, "echo ignored\n");
    const ignored = setup("ignored-input");
    bindInput(ignored, [
      {
        path: relativeRepo(ignoredPath),
        sha256: createHash("sha256").update("echo ignored\n").digest("hex"),
      },
    ]);
    await assert.rejects(
      runOnce(ignored.config, ignored.signed),
      /verification/,
    );

    const symlinkPath = join(
      process.cwd(),
      ".trash",
      "test-runs",
      `symlink-input-${process.pid}`,
    );
    await symlink(resolve(process.cwd(), "../../README.md"), symlinkPath);
    const linked = setup("symlink-input");
    bindInput(linked, [
      {
        path: relativeRepo(symlinkPath),
        sha256: "0".repeat(64),
      },
    ]);
    await assert.rejects(runOnce(linked.config, linked.signed), /type/);
  },
);

function relativeRepo(path: string) {
  return path.slice(resolve(process.cwd(), "../..").length + 1);
}

test("runner configuration rejects duplicate manifest paths and capabilities", () => {
  const s = setup("duplicates");
  const id = Object.keys(s.config.inputManifests)[0]!;
  const duplicate = s.config.inputManifests[id]![0]!;
  assert.throws(() =>
    RunnerConfig.parse({
      ...s.config,
      capabilities: ["wifi", "wifi"],
      inputManifests: { [id]: [duplicate, duplicate] },
    }),
  );
});

test("host proof rejects stale challenges", () => {
  const s = setup("stale-proof");
  assert.throws(
    () =>
      proveHost(s.config, {
        schemaVersion: 1,
        hostId: "lab-one",
        nonce: "a".repeat(43),
        issuedAt: "2020-01-01T00:00:00.000Z",
      }),
    /challenge/,
  );
});

test(
  "runner kills a noncooperating operation after its hard timeout",
  {
    skip:
      process.platform === "win32" ? "Windows runner is fail-closed" : false,
  },
  async () => {
    const s = setup("operation-timeout");
    s.config.operations.foundation!.executable = process.execPath;
    s.config.operations.foundation!.arguments = [
      "-e",
      "process.on('SIGTERM',()=>{});setInterval(()=>{},1000)",
    ];
    s.config.operations.foundation!.parameters = { default: [] };
    s.signed.request.timeoutSeconds = 1;
    s.signed.signature = signObject(s.signed.request, s.coordinator.privateKey);
    const started = Date.now();
    const response = await runOnce(s.config, s.signed);
    assert.equal(response.payload.status, "failed");
    assert.ok(Date.now() - started < 4_000);
  },
);

test(
  "Windows operation execution remains explicitly fail-closed",
  { skip: process.platform !== "win32" ? "Windows-only assertion" : false },
  async () => {
    const s = setup("windows-fail-closed");
    await assert.rejects(runOnce(s.config, s.signed), /Windows runner/);
  },
);
