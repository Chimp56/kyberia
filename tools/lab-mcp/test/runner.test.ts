import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import { join, resolve } from "node:path";
import { test } from "node:test";
import { execFileSync } from "node:child_process";
import type { SignedJobRequest } from "../src/manager.js";
import { proveHost, runOnce } from "../src/runner.js";
import type { LabRunnerConfig } from "../src/schema.js";
import { canonical, signObject, verifyObject } from "../src/security.js";
import { keys } from "./helpers.js";

const root = join(
  process.cwd(),
  ".trash",
  "test-runs",
  `lab-runner-${process.pid}`,
);
await mkdir(root, { recursive: true });
function setup(name: string) {
  const coordinator = keys(`RUNNER_COORD_${name.toUpperCase()}`),
    host = keys(`RUNNER_HOST_${name.toUpperCase()}`);
  const sha = execFileSync("git", ["rev-parse", "HEAD"], {
    cwd: resolve(process.cwd(), "../.."),
    encoding: "utf8",
  }).trim();
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
    limits: { timeoutSeconds: 5, outputBytes: 1024 },
    operations: {
      foundation: {
        executable: "/usr/bin/printf",
        arguments: [],
        version: "foundation-v1",
        parameters: { default: ["validated"] },
      },
      "probe-kismet": {
        executable: "/usr/bin/printf",
        arguments: [],
        version: "kismet-v1",
        parameters: { "expected_version=2025.01": ["kismet-ok"] },
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
  };
  const signed: SignedJobRequest = {
    request,
    signature: signObject(request, coordinator.privateKey),
    algorithm: "Ed25519",
  };
  return { config, signed, host, coordinator };
}
test("runner verifies coordinator, immutable checkout, allowlist and signs result", async () => {
  const s = setup("valid");
  const response = await runOnce(s.config, s.signed);
  assert.equal(response.payload.stdout, "validated");
  assert.equal(
    verifyObject(response.payload, response.signature, s.host.privateKey),
    true,
  );
});
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
test("runner rejects replay, tampering, stale request and revision mismatch", async () => {
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
});
test("runner maps parameter selector to static args and rejects other values", async () => {
  const s = setup("params");
  s.signed.request.suite = "probe-kismet";
  s.signed.request.parameters = { expected_version: "2025.01" };
  s.signed.signature = signObject(s.signed.request, s.coordinator.privateKey);
  const response = await runOnce(s.config, s.signed);
  assert.equal(response.payload.stdout, "kismet-ok");
});
