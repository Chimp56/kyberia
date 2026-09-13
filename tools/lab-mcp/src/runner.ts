import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { isAbsolute, relative, resolve } from "node:path";
import { RunnerConfig, type LabRunnerConfig } from "./schema.js";
import type {
  HostChallenge,
  JobRequest,
  RunnerPayload,
  SignedJobRequest,
} from "./manager.js";
import {
  canonical,
  digest,
  privateKeyFromEnv,
  publicKeyFromEnv,
  signObject,
  verifyExecutable,
  verifyObject,
} from "./security.js";

async function readStdin(limit: number): Promise<string> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of process.stdin) {
    const bytes = Buffer.from(chunk);
    size += bytes.length;
    if (size > limit) throw new Error("request too large");
    chunks.push(bytes);
  }
  return Buffer.concat(chunks).toString("utf8");
}
async function load(path: string): Promise<LabRunnerConfig> {
  return RunnerConfig.parse(JSON.parse(await readFile(resolve(path), "utf8")));
}
function selector(parameters: Record<string, string>) {
  return (
    Object.entries(parameters)
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([key, value]) => `${key}=${value}`)
      .join("&") || "default"
  );
}
async function claimNonce(config: LabRunnerConfig, request: JobRequest) {
  const issued = Date.parse(request.issuedAt),
    now = Date.now();
  if (
    !Number.isFinite(issued) ||
    Math.abs(now - issued) > config.maximumClockSkewSeconds * 1000
  )
    throw new Error("request outside freshness window");
  await mkdir(resolve(config.replayDirectory), {
    recursive: true,
    mode: 0o700,
  });
  const name = createHash("sha256").update(request.nonce).digest("hex");
  await writeFile(resolve(config.replayDirectory, name), request.runId, {
    flag: "wx",
    mode: 0o600,
  });
}
async function gitBuffer(
  config: LabRunnerConfig,
  args: string[],
  limit = config.limits.inputBytes,
): Promise<Buffer> {
  return await new Promise((done, reject) => {
    const child = spawn(config.gitExecutable, args, {
      cwd: resolve(config.checkoutDirectory),
      shell: false,
      env: {},
      stdio: ["ignore", "pipe", "ignore"],
    });
    const chunks: Buffer[] = [];
    let bytes = 0;
    child.stdout?.on("data", (chunk: Buffer) => {
      bytes += chunk.length;
      if (bytes <= limit) chunks.push(chunk);
      else child.kill("SIGKILL");
    });
    child.on("close", (code) =>
      code === 0 && bytes <= limit
        ? done(Buffer.concat(chunks, bytes))
        : reject(new Error("checkout verification failed")),
    );
    child.on("error", reject);
  });
}
type InputEntry = { path: string; sha256: string; mode: "100644" | "100755" };
export function inputManifestId(entries: InputEntry[]) {
  return digest(
    canonical([...entries].sort((a, b) => a.path.localeCompare(b.path))),
  );
}
async function completeTree(
  config: LabRunnerConfig,
  revision: string,
): Promise<Array<InputEntry & { object: string; bytes: Buffer }>> {
  const listing = await gitBuffer(
    config,
    ["ls-tree", "-rz", "--full-tree", revision],
    16_777_216,
  );
  const records = listing.toString("utf8").split("\0").filter(Boolean);
  if (!records.length || records.length > 20_000)
    throw new Error("commit tree file count rejected");
  const entries: Array<InputEntry & { object: string; bytes: Buffer }> = [];
  let total = 0;
  for (const record of records) {
    const match = /^(\d{6}) (\w+) ([0-9a-f]{40,64})\t(.+)$/.exec(record);
    if (!match) throw new Error("commit tree record rejected");
    const [, mode, type, object, path] = match;
    if (
      type !== "blob" ||
      (mode !== "100644" && mode !== "100755") ||
      !path ||
      path.startsWith("/") ||
      path.split("/").includes("..")
    )
      throw new Error("symlink, submodule, or unsafe tree entry rejected");
    const bytes = await gitBuffer(config, ["cat-file", "blob", object!]);
    total += bytes.length;
    if (total > config.limits.inputBytes)
      throw new Error("commit tree byte limit exceeded");
    entries.push({
      path,
      mode,
      object: object!,
      bytes,
      sha256: createHash("sha256").update(bytes).digest("hex"),
    });
  }
  return entries.sort((a, b) => a.path.localeCompare(b.path));
}
async function materializeInputs(config: LabRunnerConfig, request: JobRequest) {
  const operation = config.operations[request.suite];
  if (!operation || operation.inputManifestId !== request.inputManifestId)
    throw new Error("input manifest binding rejected");
  const expected = config.inputManifests[request.inputManifestId];
  if (!expected || inputManifestId(expected) !== request.inputManifestId)
    throw new Error("input manifest identity invalid");
  const tree = await completeTree(config, request.gitSha);
  const actual = tree.map(({ path, sha256, mode }) => ({ path, sha256, mode }));
  if (
    inputManifestId(actual) !== request.inputManifestId ||
    canonical(actual) !==
      canonical([...expected].sort((a, b) => a.path.localeCompare(b.path)))
  )
    throw new Error("complete commit tree manifest mismatch");
  const snapshot = resolve(config.replayDirectory, "snapshots", request.runId);
  await mkdir(resolve(config.replayDirectory, "snapshots"), {
    recursive: true,
    mode: 0o700,
  });
  await mkdir(snapshot, { mode: 0o700 });
  for (const entry of tree) {
    const target = resolve(snapshot, entry.path);
    const rel = relative(snapshot, target);
    if (!rel || rel.startsWith("..") || isAbsolute(rel))
      throw new Error("snapshot path rejected");
    await mkdir(resolve(target, ".."), { recursive: true, mode: 0o700 });
    await writeFile(target, entry.bytes, {
      flag: "wx",
      mode: entry.mode === "100755" ? 0o500 : 0o400,
    });
  }
  return snapshot;
}
async function execute(config: LabRunnerConfig, request: JobRequest) {
  const operation = config.operations[request.suite];
  if (!operation) throw new Error("operation not allowlisted");
  const staticArgs = operation.parameters[selector(request.parameters)];
  if (!staticArgs) throw new Error("parameter combination not allowlisted");
  if (request.timeoutSeconds > config.limits.timeoutSeconds)
    throw new Error("timeout rejected");
  if (request.specVersion !== operation.version)
    throw new Error("operation version mismatch");
  const snapshot = await materializeInputs(config, request);
  await verifyExecutable(operation.executable, operation.executableSha256);
  return await new Promise<{
    stdout: string;
    stderr: string;
    status: "succeeded" | "failed";
  }>((done, reject) => {
    const child = spawn(
      operation.executable,
      [...operation.arguments, ...staticArgs],
      {
        cwd: snapshot,
        shell: false,
        detached: true,
        env: {
          PATH: "",
          KYBERIA_LAB_SEED: String(request.seed),
          KYBERIA_LAB_SPEC_VERSION: operation.version,
          KYBERIA_LAB_INPUT_MANIFEST_ID: request.inputManifestId,
        },
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
    const stdout: Buffer[] = [],
      stderr: Buffer[] = [];
    let stdoutBytes = 0,
      stderrBytes = 0,
      size = 0,
      settled = false,
      stopping = false;
    let hardTimer: NodeJS.Timeout | undefined;
    const finish = (
      error?: Error,
      result?: {
        stdout: string;
        stderr: string;
        status: "succeeded" | "failed";
      },
    ) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      clearTimeout(hardTimer);
      if (error) reject(error);
      else done(result!);
    };
    const stop = () => {
      if (settled || stopping) return;
      stopping = true;
      try {
        if (child.pid !== undefined) process.kill(-child.pid, "SIGTERM");
      } catch {}
      hardTimer = setTimeout(() => {
        if (!settled && child.pid !== undefined)
          try {
            process.kill(-child.pid, "SIGKILL");
          } catch {}
        setTimeout(
          () => finish(new Error("command did not terminate after deadline")),
          500,
        ).unref();
      }, 2000).unref();
    };
    const add = (which: "stdout" | "stderr", chunk: Buffer) => {
      size += chunk.length;
      if (size > config.limits.outputBytes) {
        stop();
        return;
      }
      if (which === "stdout") {
        stdoutBytes += chunk.length;
        stdout.push(chunk);
      } else {
        stderrBytes += chunk.length;
        stderr.push(chunk);
      }
    };
    child.stdout?.on("data", (c: Buffer) => add("stdout", c));
    child.stderr?.on("data", (c: Buffer) => add("stderr", c));
    const timer = setTimeout(stop, request.timeoutSeconds * 1000);
    timer.unref();
    child.on("error", (error) => finish(error));
    child.on("close", (code) => {
      if (size > config.limits.outputBytes)
        finish(new Error("command output exceeded limit"));
      else
        finish(undefined, {
          stdout: Buffer.concat(stdout, stdoutBytes).toString("utf8"),
          stderr: Buffer.concat(stderr, stderrBytes).toString("utf8"),
          status: code === 0 ? "succeeded" : "failed",
        });
    });
  });
}
export async function runOnce(
  config: LabRunnerConfig,
  signed: SignedJobRequest,
) {
  if (process.platform === "win32")
    throw new Error(
      "Windows runner requires approved native Job Object containment",
    );
  await verifyExecutable(config.gitExecutable, config.gitExecutableSha256);
  if (
    signed.algorithm !== "Ed25519" ||
    signed.request.coordinatorKeyId !== config.coordinatorKeyId
  )
    throw new Error("coordinator identity rejected");
  if (
    !verifyObject(
      signed.request,
      signed.signature,
      publicKeyFromEnv(config.coordinatorPublicKeyEnv),
    )
  )
    throw new Error("coordinator signature invalid");
  if (signed.request.hostId !== config.hostId)
    throw new Error("host binding rejected");
  const operation = config.operations[signed.request.suite];
  if (
    !operation ||
    signed.request.specVersion !== operation.version ||
    signed.request.inputManifestId !== operation.inputManifestId
  )
    throw new Error("signed operation specification rejected");
  await claimNonce(config, signed.request);
  const startedAt = new Date().toISOString();
  const result = await execute(config, signed.request);
  const payload: RunnerPayload = {
    schemaVersion: 1,
    requestDigest: digest(canonical(signed)),
    hostId: config.hostId,
    status: result.status,
    startedAt,
    finishedAt: new Date().toISOString(),
    stdout: result.stdout,
    stderr: result.stderr,
    capabilities: [...config.capabilities].sort(),
    toolIdentities: [
      {
        role: "git" as const,
        path: config.gitExecutable,
        sha256: config.gitExecutableSha256,
      },
      {
        role: "operation" as const,
        path: operation.executable,
        sha256: operation.executableSha256,
      },
    ],
  };
  const signedPreimage = {
    schemaVersion: 1 as const,
    payloadDigest: digest(canonical(payload)),
  };
  return {
    payload,
    signedPreimage,
    signature: signObject(
      signedPreimage,
      privateKeyFromEnv(config.hostPrivateKeyEnv),
    ),
    algorithm: "Ed25519" as const,
  };
}
export function proveHost(config: LabRunnerConfig, challenge: HostChallenge) {
  const issuedAt = Date.parse(challenge.issuedAt);
  if (
    challenge.schemaVersion !== 1 ||
    challenge.hostId !== config.hostId ||
    !/^[A-Za-z0-9_-]{43}$/.test(challenge.nonce) ||
    !Number.isFinite(issuedAt) ||
    Math.abs(Date.now() - issuedAt) > config.maximumClockSkewSeconds * 1000
  )
    throw new Error("invalid host challenge");
  const payload = {
    schemaVersion: 1 as const,
    hostId: config.hostId,
    challengeDigest: digest(canonical(challenge)),
    capabilities: [...config.capabilities].sort(),
  };
  return {
    payload,
    signature: signObject(payload, privateKeyFromEnv(config.hostPrivateKeyEnv)),
    algorithm: "Ed25519" as const,
  };
}
if (import.meta.url === new URL(process.argv[1] ?? "", "file:").href) {
  const path = process.env.KYBERIA_LAB_RUNNER_CONFIG;
  if (!path) throw new Error("KYBERIA_LAB_RUNNER_CONFIG is required");
  const config = await load(path);
  const input = JSON.parse(await readStdin(65536));
  const result =
    input.kind === "authenticate"
      ? proveHost(config, input.challenge as HostChallenge)
      : await runOnce(config, input as SignedJobRequest);
  process.stdout.write(`${canonical(result)}\n`);
}
