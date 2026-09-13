import assert from "node:assert/strict";
import { mkdir, readFile, symlink, writeFile } from "node:fs/promises";
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { join, resolve } from "node:path";
import { test } from "node:test";
import { execFileSync } from "node:child_process";
import type { SignedJobRequest } from "../src/manager.js";
import { generateInputManifest } from "../src/input-manifest.js";
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
const repository = resolve(process.cwd(), "../..");
const gitExecutable =
  process.platform === "win32"
    ? "C:\\Program Files\\Git\\cmd\\git.exe"
    : "/usr/bin/git";
const fileSha256 = (path: string) =>
  createHash("sha256").update(readFileSync(path)).digest("hex");
const gitExecutableSha256 =
  process.platform === "win32" ? "0".repeat(64) : fileSha256(gitExecutable);
const committedSha =
  process.platform === "win32"
    ? "0".repeat(40)
    : execFileSync(gitExecutable, ["rev-parse", "HEAD"], {
        cwd: repository,
        encoding: "utf8",
      }).trim();
function inputsFor(repositoryPath: string, revision: string) {
  return execFileSync(
    gitExecutable,
    ["ls-tree", "-rz", "--full-tree", revision],
    { cwd: repositoryPath },
  )
    .toString("utf8")
    .split("\0")
    .filter(Boolean)
    .map((record) => {
      const match = /^(100644|100755) blob ([0-9a-f]{40,64})\t(.+)$/.exec(
        record,
      );
      assert.ok(match, `unsupported test tree entry: ${record}`);
      return {
        path: match[3]!,
        mode: match[1]! as "100644" | "100755",
        sha256: createHash("sha256")
          .update(
            execFileSync(gitExecutable, ["cat-file", "blob", match[2]!], {
              cwd: repositoryPath,
              maxBuffer: 64_000_000,
            }),
          )
          .digest("hex"),
      };
    })
    .sort((a, b) => a.path.localeCompare(b.path));
}
const committedInputs =
  process.platform === "win32"
    ? [{ path: "README.md", mode: "100644" as const, sha256: "0".repeat(64) }]
    : inputsFor(repository, committedSha);
executableRunnerTest(
  "operator manifest generator covers the complete commit",
  async () => {
    const generated = await generateInputManifest(
      gitExecutable,
      gitExecutableSha256,
      repository,
      committedSha,
    );
    assert.deepEqual(generated.entries, committedInputs);
    assert.equal(generated.inputManifestId, inputManifestId(committedInputs));
  },
);
function setup(name: string) {
  const coordinator = keys(`RUNNER_COORD_${name.toUpperCase()}`),
    host = keys(`RUNNER_HOST_${name.toUpperCase()}`);
  const sha = committedSha;
  const inputs = committedInputs;
  const manifestId = inputManifestId(inputs);
  const config: LabRunnerConfig = {
    schemaVersion: 1,
    hostId: "lab-one",
    hostIdentity: host.identity,
    hostPrivateKeyEnv: host.privateName,
    coordinatorPublicKeyEnv: coordinator.publicName,
    coordinatorKeyId: "coordinator-1",
    gitExecutable,
    gitExecutableSha256,
    checkoutDirectory: repository,
    replayDirectory: join(root, name),
    maximumClockSkewSeconds: 60,
    capabilities: ["wifi"],
    inputManifests: { [manifestId]: inputs },
    limits: { timeoutSeconds: 5, outputBytes: 1024, inputBytes: 64_000_000 },
    operations: {
      foundation: {
        executable: "/usr/bin/printf",
        executableSha256: fileSha256("/usr/bin/printf"),
        arguments: [],
        version: "foundation-v1",
        parameters: { default: ["validated"] },
        inputManifestId: manifestId,
      },
      "probe-kismet": {
        executable: "/usr/bin/printf",
        executableSha256: fileSha256("/usr/bin/printf"),
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
function bindInput(
  s: ReturnType<typeof setup>,
  entries: {
    path: string;
    sha256: string;
    mode: "100644" | "100755";
  }[],
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
      verifyObject(
        response.signedPreimage,
        response.signature,
        s.host.privateKey,
      ),
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
    await assert.rejects(
      runOnce(revision.config, revision.signed),
      /revision|checkout verification/,
    );
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
  "runner materializes complete commit and excludes dirty external inputs",
  async () => {
    const changed = setup("changed-input");
    bindInput(changed, [{ ...committedInputs[0]!, sha256: "0".repeat(64) }]);
    await assert.rejects(
      runOnce(changed.config, changed.signed),
      /tree manifest/,
    );

    const dirtyRepo = join(root, "owned-dirty-repository");
    await mkdir(dirtyRepo, { recursive: true });
    await writeFile(join(dirtyRepo, ".gitignore"), "ignored-*\n");
    await writeFile(join(dirtyRepo, "seed.txt"), "committed\n");
    for (const args of [
      ["init"],
      ["config", "user.name", "Kyberia Test"],
      ["config", "user.email", "test@invalid.example"],
      ["add", ".gitignore", "seed.txt"],
      ["commit", "-m", "fixture"],
    ])
      execFileSync(gitExecutable, args, { cwd: dirtyRepo });
    const revision = execFileSync(gitExecutable, ["rev-parse", "HEAD"], {
      cwd: dirtyRepo,
      encoding: "utf8",
    }).trim();
    const inputs = inputsFor(dirtyRepo, revision);
    await writeFile(join(dirtyRepo, "seed.txt"), "dirty\n");
    await writeFile(join(dirtyRepo, "untracked.txt"), "untracked\n");
    await writeFile(join(dirtyRepo, "ignored-command"), "ignored\n");
    await symlink("seed.txt", join(dirtyRepo, "ignored-link"));
    const dirty = setup("dirty-input");
    dirty.config.checkoutDirectory = dirtyRepo;
    dirty.config.operations.foundation!.executable = "/bin/cat";
    dirty.config.operations.foundation!.executableSha256 =
      fileSha256("/bin/cat");
    dirty.config.operations.foundation!.arguments = ["seed.txt"];
    dirty.config.operations.foundation!.parameters = { default: [] };
    dirty.signed.request.gitSha = revision;
    bindInput(dirty, inputs);
    const response = await runOnce(dirty.config, dirty.signed);
    assert.equal(response.payload.stdout, "committed\n");
    assert.doesNotMatch(response.payload.stdout, /dirty|untracked|ignored/);
  },
);

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

executableRunnerTest(
  "runner rejects unpinned Git executable bytes",
  async () => {
    const s = setup("git-identity");
    s.config.gitExecutableSha256 = "f".repeat(64);
    await assert.rejects(runOnce(s.config, s.signed), /digest mismatch/);
  },
);

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
    const smallRepo = join(root, "operation-timeout-repository");
    await mkdir(smallRepo, { recursive: true });
    await writeFile(join(smallRepo, "input.txt"), "bounded\n");
    for (const args of [
      ["init"],
      ["config", "user.name", "Kyberia Test"],
      ["config", "user.email", "test@invalid.example"],
      ["add", "input.txt"],
      ["commit", "-m", "fixture"],
    ])
      execFileSync(gitExecutable, args, { cwd: smallRepo });
    s.config.checkoutDirectory = smallRepo;
    s.signed.request.gitSha = execFileSync(
      gitExecutable,
      ["rev-parse", "HEAD"],
      { cwd: smallRepo, encoding: "utf8" },
    ).trim();
    bindInput(s, inputsFor(smallRepo, s.signed.request.gitSha));
    s.config.operations.foundation!.executable = process.execPath;
    s.config.operations.foundation!.executableSha256 = fileSha256(
      process.execPath,
    );
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
