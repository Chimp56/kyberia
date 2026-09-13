import assert from "node:assert/strict";
import { mkdir, rename, symlink, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { test } from "node:test";
import type {
  Executor,
  HostChallenge,
  SignedJobRequest,
} from "../src/manager.js";
import { LabManager } from "../src/manager.js";
import { canonical, digest, signObject } from "../src/security.js";
import { config } from "./helpers.js";

const root = join(
  process.cwd(),
  ".trash",
  "test-runs",
  `lab-mcp-${process.pid}`,
);
await mkdir(root, { recursive: true });

class Fake implements Executor {
  requests: SignedJobRequest[] = [];
  delay = 0;
  wrongKey = false;
  constructor(
    private readonly privateKey: ReturnType<
      typeof config
    >["hostKeys"]["privateKey"],
  ) {}
  async authenticate(_spec: unknown, challenge: HostChallenge) {
    const payload = {
      schemaVersion: 1 as const,
      hostId: challenge.hostId,
      challengeDigest: digest(canonical(challenge)),
      capabilities: ["wifi", "cuda"],
    };
    return canonical({
      payload,
      signature: signObject(payload, this.privateKey),
      algorithm: "Ed25519",
    });
  }
  async execute(
    _spec: unknown,
    request: SignedJobRequest,
    _timeout: number,
    signal: AbortSignal,
  ) {
    this.requests.push(request);
    if (this.delay)
      await new Promise<void>((done) => {
        const timer = setTimeout(done, this.delay);
        signal.addEventListener(
          "abort",
          () => {
            clearTimeout(timer);
            done();
          },
          { once: true },
        );
      });
    const payload = {
      schemaVersion: 1 as const,
      requestDigest: digest(canonical(request)),
      hostId: request.request.hostId,
      status: signal.aborted ? ("cancelled" as const) : ("succeeded" as const),
      startedAt: "2026-09-13T00:00:00.000Z",
      finishedAt: "2026-09-13T00:00:01.000Z",
      stdout:
        "SSID=secret\nAA:BB:CC:DD:EE:FF 192.168.1.2 token=hunter2\n" +
        "x".repeat(2000),
      stderr: "",
      capabilities: ["wifi", "cuda"],
    };
    return canonical({
      payload,
      signature: signObject(payload, this.privateKey),
      algorithm: "Ed25519",
    });
  }
}
async function waitDone(manager: LabManager, id: string) {
  for (let i = 0; i < 100; i++) {
    const s = manager.status(id).status;
    if (!["queued", "running", "cancelling"].includes(s)) return;
    await new Promise((done) => setTimeout(done, 5));
  }
  throw new Error("run did not finish");
}

test("immutable revision, allowlists and strict identifiers reject hostile input", async () => {
  const setup = config(join(root, "admission"));
  const manager = new LabManager(
    setup.config,
    new Fake(setup.hostKeys.privateKey),
  );
  await assert.rejects(manager.submit("lab-one", "main", "foundation", 1, 2));
  await assert.rejects(
    manager.submit("lab-one", "f".repeat(40), "foundation", 1, 2),
  );
  await assert.rejects(
    manager.probe("lab-one", "kismet", { expected_version: "../../evil" }),
  );
  await assert.rejects(
    manager.probe("lab-one", "sionna", { scene_set: "other" }),
  );
  assert.throws(() => manager.status("../run"));
});

test("signed request binds revision and result; output is redacted and bounded", async () => {
  const setup = config(join(root, "signed"));
  const fake = new Fake(setup.hostKeys.privateKey);
  const manager = new LabManager(setup.config, fake);
  const id = await manager.submit(
    "lab-one",
    setup.config.immutableRevisions[0]!,
    "foundation",
    42,
    10,
  );
  await waitDone(manager, id);
  assert.equal(manager.status(id).status, "succeeded");
  assert.equal(
    fake.requests[0]!.request.gitSha,
    setup.config.immutableRevisions[0],
  );
  assert.equal(fake.requests[0]!.request.seed, 42);
  assert.equal(manager.verifyManifest(id, "lab-one"), true);
  assert.equal(manager.verifyManifest(id, "other"), false);
  const artifact = await manager.fetch(id, "stdout.txt");
  assert.ok(artifact.bytes <= 1024);
  assert.match(artifact.content, /redacted-mac/);
  assert.doesNotMatch(artifact.content, /hunter2|192\.168/);
  const signed = manager.manifest(id);
  signed.manifest.seed = 99;
  assert.equal(manager.verifyManifest(id), false);
});

test("wrong host key fails before work", async () => {
  const setup = config(join(root, "auth"));
  const attacker = config(join(root, "attacker"));
  const manager = new LabManager(
    setup.config,
    new Fake(attacker.hostKeys.privateKey),
  );
  await assert.rejects(
    manager.submit(
      "lab-one",
      setup.config.immutableRevisions[0]!,
      "foundation",
      1,
      3,
    ),
    /identity|signature/,
  );
});

test("running and queued cancellation finish with signed manifests", async () => {
  const setup = config(join(root, "cancel"));
  const fake = new Fake(setup.hostKeys.privateKey);
  fake.delay = 100;
  const manager = new LabManager(setup.config, fake);
  const first = await manager.submit(
    "lab-one",
    setup.config.immutableRevisions[0]!,
    "foundation",
    1,
    5,
  );
  const second = await manager.submit(
    "lab-one",
    setup.config.immutableRevisions[0]!,
    "foundation",
    2,
    5,
  );
  manager.cancel(second);
  manager.cancel(first);
  await waitDone(manager, first);
  await waitDone(manager, second);
  assert.equal(manager.manifest(first).manifest.status, "cancelled");
  assert.equal(manager.verifyManifest(first), true);
  assert.equal(manager.manifest(second).manifest.status, "cancelled");
});

test("artifact traversal, tampering and symlinks are rejected", async () => {
  const setup = config(join(root, "artifact"));
  const manager = new LabManager(
    setup.config,
    new Fake(setup.hostKeys.privateKey),
  );
  const id = await manager.submit(
    "lab-one",
    setup.config.immutableRevisions[0]!,
    "foundation",
    1,
    2,
  );
  await waitDone(manager, id);
  await assert.rejects(manager.fetch(id, "../../secret"));
  await writeFile(
    join(setup.config.stateDirectory, id, "stdout.txt"),
    "tampered",
  );
  await assert.rejects(manager.fetch(id, "stdout.txt"), /integrity/);
  const external = join(root, "external");
  await writeFile(external, "x");
  const stderrPath = join(setup.config.stateDirectory, id, "stderr.txt");
  await rename(
    stderrPath,
    join(setup.config.stateDirectory, id, "stderr-original.txt"),
  );
  await symlink(external, stderrPath);
  await assert.rejects(manager.fetch(id, "stderr.txt"), /type/);
});
