import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { mkdir, readFile, rename, symlink, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { test } from "node:test";
import type {
  Executor,
  HostChallenge,
  SignedJobRequest,
} from "../src/manager.js";
import {
  LabManager,
  ProcessExecutor,
  verifyPersistedManifest,
} from "../src/manager.js";
import { Config } from "../src/schema.js";
import {
  canonical,
  digest,
  publicKeyFromEnv,
  publicKeyIdentity,
  signObject,
  sanitize,
  verifyObject,
} from "../src/security.js";
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
  authDelay = 0;
  finishedOffsetMs = 0;
  beforeReturn?: (request: SignedJobRequest) => Promise<void>;
  constructor(
    private readonly privateKey: ReturnType<
      typeof config
    >["hostKeys"]["privateKey"],
  ) {}
  async authenticate(_spec: unknown, challenge: HostChallenge) {
    if (this.authDelay)
      await new Promise((done) => setTimeout(done, this.authDelay));
    const payload = {
      schemaVersion: 1 as const,
      hostId: challenge.hostId,
      challengeDigest: digest(canonical(challenge)),
      capabilities: ["wifi", "cpu-llvm"],
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
      startedAt: request.request.issuedAt,
      finishedAt: new Date(
        Date.parse(request.request.issuedAt) + this.finishedOffsetMs,
      ).toISOString(),
      stdout: sanitize(
        "SSID=secret\nAA:BB:CC:DD:EE:FF 192.168.1.2 token=hunter2\n" +
          "x".repeat(2000),
        1024,
      ).text,
      stderr: "",
      sanitization: "kyberia-lab-text-v2" as const,
      capabilities: ["wifi", "cpu-llvm"],
      toolIdentities: [
        {
          role: "git" as const,
          id: "git-test",
          version: "1",
          sha256: "0".repeat(64),
        },
        {
          role: "operation" as const,
          id: "operation-test",
          version: "1",
          sha256: "1".repeat(64),
        },
      ],
    };
    await this.beforeReturn?.(request);
    const signedPreimage = {
      schemaVersion: 1 as const,
      payloadDigest: digest(canonical(payload)),
    };
    return canonical({
      payload,
      signedPreimage,
      signature: signObject(signedPreimage, this.privateKey),
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
  await assert.rejects(
    manager.probe("lab-one", "sionna", { cpu_or_gpu: "gpu" }),
    /unsupported probe parameter/,
  );
  assert.throws(() => manager.status("../run"));
});

test("configuration forbids forwarding the coordinator private key", () => {
  const setup = config(join(root, "private-key-forwarding"));
  setup.config.hosts[0]!.suites.foundation!.credentialEnvNames = [
    setup.config.manifestPrivateKeyEnv,
  ];
  assert.throws(() => Config.parse(setup.config), /cannot be forwarded/);
  const duplicate = config(join(root, "duplicate-config"));
  duplicate.config.hosts.push({ ...duplicate.config.hosts[0]! });
  assert.throws(() => Config.parse(duplicate.config));
  const allowlist = config(join(root, "duplicate-allowlist"));
  allowlist.config.hosts[0]!.allowedFixtureSets = ["golden-v1", "golden-v1"];
  assert.throws(() => Config.parse(allowlist.config));
  for (const bypass of [
    ["-e", "code"],
    ["--eval=code"],
    ["-lc", "code"],
    ["-m", "module"],
    ["--loader", "relative-loader.mjs"],
    ["--import=relative-loader.mjs"],
    ["relative-runner.mjs"],
    ["--require", "relative-runner.cjs"],
  ]) {
    const raw = JSON.parse(JSON.stringify(setup.config));
    raw.hosts[0].suites.foundation.arguments = bypass;
    assert.throws(() => Config.parse(raw));
  }
  const retiredInvocation = JSON.parse(JSON.stringify(setup.config));
  retiredInvocation.hosts[0].suites.foundation.invocation = {
    kind: "node-bundle",
    bundlePath: "/absolute/runner.mjs",
    bundleSha256: "0".repeat(64),
  };
  assert.throws(() => Config.parse(retiredInvocation));
  const retiredSuite = JSON.parse(JSON.stringify(setup.config));
  retiredSuite.hosts[0].suites["sionna-gpu"] =
    retiredSuite.hosts[0].suites.foundation;
  assert.throws(() => Config.parse(retiredSuite));
  for (const retiredCapability of [
    "cuda",
    "cuda12",
    "gpu",
    "gpu0",
    "nvidia",
    "nvidia-gpu",
    "sionna-cuda",
    "sionna-gpu",
    "wgpu-cuda",
  ]) {
    const retiredHost = JSON.parse(JSON.stringify(setup.config));
    retiredHost.hosts[0].capabilities.push(retiredCapability);
    assert.throws(
      () => Config.parse(retiredHost),
      /retired accelerator capabilities/,
    );
  }
  const portableCompute = JSON.parse(JSON.stringify(setup.config));
  portableCompute.hosts[0].capabilities.push("wgpu");
  assert.doesNotThrow(() => Config.parse(portableCompute));
});

test("coordinator and host key fingerprints must be distinct and paired", () => {
  const shared = config(join(root, "shared-key"));
  shared.config.hosts[0]!.publicKeyEnv = shared.config.manifestPublicKeyEnv;
  shared.config.hosts[0]!.identity = publicKeyIdentity(
    publicKeyFromEnv(shared.config.manifestPublicKeyEnv),
  );
  assert.throws(() => new LabManager(shared.config), /distinct/);
  const mismatch = config(join(root, "coordinator-mismatch"));
  const another = config(join(root, "other-coordinator"));
  mismatch.config.manifestPrivateKeyEnv = another.config.manifestPrivateKeyEnv;
  assert.throws(() => new LabManager(mismatch.config), /pair mismatch/);
});

test("authentication reservations bound concurrent submissions", async () => {
  const setup = config(join(root, "reservations"));
  const fake = new Fake(setup.hostKeys.privateKey);
  fake.authDelay = 30;
  const manager = new LabManager(setup.config, fake);
  const results = await Promise.allSettled(
    Array.from({ length: 12 }, (_, seed) =>
      manager.submit(
        "lab-one",
        setup.config.immutableRevisions[0]!,
        "foundation",
        seed,
        2,
      ),
    ),
  );
  assert.ok(
    results.filter((result) => result.status === "fulfilled").length <= 3,
  );
  assert.ok(results.some((result) => result.status === "rejected"));
});

test("restart recovery turns interrupted intent into signed failed evidence", async () => {
  const setup = config(join(root, "recovery"));
  const fake = new Fake(setup.hostKeys.privateKey);
  fake.delay = 200;
  const manager = new LabManager(setup.config, fake);
  const id = await manager.submit(
    "lab-one",
    setup.config.immutableRevisions[0]!,
    "foundation",
    7,
    2,
  );
  while (manager.status(id).status !== "running")
    await new Promise((done) => setTimeout(done, 2));
  await new Promise((done) => setTimeout(done, 10));
  const recovered = new LabManager(setup.config, fake);
  assert.equal(recovered.status(id).status, "failed");
  assert.match(recovered.status(id).error ?? "", /restarted/);
  assert.equal(recovered.manifest(id).manifest.status, "failed");
  assert.equal(recovered.verifyManifest(id), true);
  manager.cancel(id);
  await waitDone(manager, id);
});

test("restart recovery rejects malformed persisted run state", async () => {
  const setup = config(join(root, "malformed-recovery"));
  const dir = join(setup.config.stateDirectory, "run-malformed");
  await mkdir(dir, { recursive: true });
  await writeFile(
    join(dir, "status.json"),
    JSON.stringify({ id: "../../escape", status: "running" }),
  );
  const manager = new LabManager(
    setup.config,
    new Fake(setup.hostKeys.privateKey),
  );
  assert.throws(() => manager.status("run-malformed"), /unknown/);
});

test("artifact publication failure resolves to recoverable failed status", async () => {
  const setup = config(join(root, "publication-failure"));
  const fake = new Fake(setup.hostKeys.privateKey);
  fake.beforeReturn = async (request) => {
    const dir = join(setup.config.stateDirectory, request.request.runId);
    await writeFile(join(dir, "stdout.txt"), "collision");
  };
  const manager = new LabManager(setup.config, fake);
  const id = await manager.submit(
    "lab-one",
    setup.config.immutableRevisions[0]!,
    "foundation",
    1,
    2,
  );
  await waitDone(manager, id);
  assert.equal(manager.status(id).status, "failed");
  assert.match(manager.status(id).error ?? "", /publication/);
  assert.throws(() => manager.manifest(id), /unavailable/);
  const restarted = new LabManager(setup.config, fake);
  assert.equal(restarted.status(id).status, "failed");
});

test(
  "ProcessExecutor imposes a hard deadline on a noncooperating child",
  {
    skip:
      process.platform === "win32"
        ? "Windows containment is fail-closed"
        : false,
  },
  async () => {
    const setup = config(join(root, "hard-deadline"));
    const executor = new ProcessExecutor();
    const script = join(root, "hard-deadline-runner.mjs");
    await writeFile(
      script,
      "process.on('SIGTERM',()=>{});process.stdin.resume();setInterval(()=>{},1000)\n",
    );
    const spec = {
      executable: process.execPath,
      executableSha256: createHash("sha256")
        .update(readFileSync(process.execPath))
        .digest("hex"),
      invocation: {
        kind: "single-pinned-bundle" as const,
        bundlePath: script,
        bundleSha256: createHash("sha256")
          .update(await readFile(script))
          .digest("hex"),
      },
      arguments: [] as [],
      argumentFiles: [],
      version: "test-v1",
      environment: { KYBERIA_LAB_RUNNER_CONFIG: "/unused" },
      credentialEnvNames: [setup.config.manifestPublicKeyEnv],
      inputManifestId: "sha256:" + "0".repeat(64),
    };
    const controller = new AbortController();
    setTimeout(() => controller.abort(), 200);
    const started = Date.now();
    await assert.rejects(
      executor.execute(
        spec,
        {
          request: {} as SignedJobRequest["request"],
          signature: "",
          algorithm: "Ed25519",
        },
        10_000,
        controller.signal,
        1024,
      ),
      /cancelled|terminate/,
    );
    assert.ok(Date.now() - started < 3_500);
  },
);

test("ProcessExecutor rejects a changed runner bundle before launch", async () => {
  const setup = config(join(root, "argument-tamper"));
  const script = join(root, "argument-tamper", "runner.mjs");
  await mkdir(join(root, "argument-tamper"), { recursive: true });
  await writeFile(script, "process.stdout.write('original')\n");
  const expected = createHash("sha256")
    .update(await readFile(script))
    .digest("hex");
  await writeFile(script, "process.stdout.write('tampered')\n");
  const executor = new ProcessExecutor();
  await assert.rejects(
    executor.authenticate(
      {
        executable: process.execPath,
        executableSha256: createHash("sha256")
          .update(readFileSync(process.execPath))
          .digest("hex"),
        invocation: {
          kind: "single-pinned-bundle",
          bundlePath: script,
          bundleSha256: expected,
        },
        arguments: [],
        argumentFiles: [],
        version: "test-v1",
        environment: { KYBERIA_LAB_RUNNER_CONFIG: "/unused" },
        credentialEnvNames: [setup.config.manifestPublicKeyEnv],
        inputManifestId: "sha256:" + "0".repeat(64),
      },
      {
        schemaVersion: 1,
        hostId: "lab-one",
        nonce: "a".repeat(43),
        issuedAt: new Date().toISOString(),
      },
      1_000,
      1_024,
    ),
    /digest mismatch/,
  );
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
  assert.equal(signed.manifest.evidence.origin, "host-signed");
  if (signed.manifest.evidence.origin === "host-signed") {
    assert.equal(
      verifyObject(
        signed.manifest.evidence.signedPreimage,
        signed.manifest.evidence.hostSignature,
        publicKeyFromEnv(setup.config.hosts[0]!.publicKeyEnv),
      ),
      true,
    );
  }
  const persisted = JSON.parse(
    await readFile(
      join(setup.config.stateDirectory, id, "manifest.json"),
      "utf8",
    ),
  ) as typeof signed;
  assert.doesNotMatch(
    canonical(persisted.manifest),
    /\/usr\/|\/private\/|\\Users\\/,
  );
  assert.deepEqual(persisted.manifest.evidence, signed.manifest.evidence);
  if (persisted.manifest.evidence.origin === "host-signed") {
    const evidence = persisted.manifest.evidence;
    assert.equal(
      evidence.signedPreimage.payloadDigest,
      digest(canonical(evidence.hostPayload)),
    );
    assert.equal(evidence.hostPayload.hostId, persisted.manifest.hostId);
    assert.equal(
      evidence.hostPayload.requestDigest,
      persisted.manifest.requestDigest,
    );
    assert.doesNotMatch(
      evidence.hostPayload.stdout,
      /secret|hunter2|192\.168\.1\.2/,
    );
    for (const artifactMetadata of persisted.manifest.artifacts) {
      const bytes = await readFile(
        join(setup.config.stateDirectory, id, artifactMetadata.name),
      );
      assert.equal(digest(bytes), artifactMetadata.sha256);
    }
  }
  const reopened = new LabManager(setup.config, fake);
  assert.equal(reopened.verifyManifest(id, "lab-one"), true);
  const coordinatorPublic = publicKeyFromEnv(setup.config.manifestPublicKeyEnv);
  const hostPublic = publicKeyFromEnv(setup.config.hosts[0]!.publicKeyEnv);
  const artifactReader = (name: string) =>
    readFileSync(join(setup.config.stateDirectory, id, name));
  assert.equal(
    verifyPersistedManifest(
      persisted,
      coordinatorPublic,
      hostPublic,
      artifactReader,
    ),
    true,
  );
  assert.equal(
    verifyObject(
      persisted.manifest.signedRequest.request,
      persisted.manifest.signedRequest.signature,
      coordinatorPublic,
    ),
    true,
  );
  for (const [field, value] of [
    ["runId", "run-other"],
    ["hostId", "other-host"],
    ["gitSha", "f".repeat(40)],
    ["suite", "other-suite"],
    ["seed", 99],
    ["timeoutSeconds", 11],
    ["parameters", { selector: "other" }],
    ["requestNonce", "b".repeat(43)],
    ["requestIssuedAt", "2026-09-13T00:00:00.000Z"],
    ["createdAt", "2026-09-13T00:00:00.000Z"],
    ["coordinatorKeyId", "other-coordinator"],
    ["specVersion", "other-v1"],
    ["inputManifestId", "sha256:" + "f".repeat(64)],
  ] as const) {
    const mutated = structuredClone(persisted);
    (mutated.manifest as unknown as Record<string, unknown>)[field] = value;
    mutated.signature = signObject(
      mutated.manifest,
      setup.coordinatorKeys.privateKey,
    );
    assert.equal(
      verifyPersistedManifest(
        mutated,
        coordinatorPublic,
        hostPublic,
        artifactReader,
      ),
      false,
      `accepted mutated ${field}`,
    );
  }
  signed.manifest.seed = 99;
  assert.equal(manager.verifyManifest(id), false);
});

test("host results outside the requested timing envelope are rejected", async () => {
  const setup = config(join(root, "timing"));
  const fake = new Fake(setup.hostKeys.privateKey);
  fake.finishedOffsetMs = 10_000;
  const manager = new LabManager(setup.config, fake);
  const id = await manager.submit(
    "lab-one",
    setup.config.immutableRevisions[0]!,
    "foundation",
    1,
    2,
  );
  await waitDone(manager, id);
  assert.equal(manager.status(id).status, "failed");
  assert.equal(
    manager.manifest(id).manifest.evidence.origin,
    "coordinator-terminal",
  );
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
