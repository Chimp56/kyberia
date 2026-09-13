import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { lstat, mkdir, readFile, writeFile } from "node:fs/promises";
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
async function currentRevision(config: LabRunnerConfig): Promise<string> {
  return (await git(config, ["rev-parse", "HEAD"])).trim();
}
async function git(config: LabRunnerConfig, args: string[]): Promise<string> {
  return await new Promise((done, reject) => {
    const child = spawn("git", args, {
      cwd: resolve(config.checkoutDirectory),
      shell: false,
      env: { PATH: process.env.PATH ?? "" },
      stdio: ["ignore", "pipe", "ignore"],
    });
    let out = "";
    child.stdout?.on("data", (chunk) => {
      out += String(chunk).slice(0, 80);
    });
    child.on("close", (code) =>
      code === 0
        ? done(out.trim())
        : reject(new Error("checkout verification failed")),
    );
    child.on("error", reject);
  });
}
export function inputManifestId(entries: { path: string; sha256: string }[]) {
  return digest(
    canonical([...entries].sort((a, b) => a.path.localeCompare(b.path))),
  );
}
async function verifyInputs(config: LabRunnerConfig, request: JobRequest) {
  const operation = config.operations[request.suite];
  if (!operation || operation.inputManifestId !== request.inputManifestId)
    throw new Error("input manifest binding rejected");
  const entries = config.inputManifests[request.inputManifestId];
  if (!entries || inputManifestId(entries) !== request.inputManifestId)
    throw new Error("input manifest identity invalid");
  const root = resolve(config.checkoutDirectory);
  const paths = new Set<string>();
  for (const entry of entries) {
    if (paths.has(entry.path)) throw new Error("duplicate manifest input");
    paths.add(entry.path);
    const path = resolve(root, entry.path);
    const rel = relative(root, path);
    if (!rel || rel.startsWith("..") || isAbsolute(rel))
      throw new Error("manifest path escapes checkout");
    const stat = await lstat(path);
    if (!stat.isFile() || stat.isSymbolicLink())
      throw new Error("manifest input type rejected");
    const actual = createHash("sha256")
      .update(await readFile(path))
      .digest("hex");
    if (actual !== entry.sha256) throw new Error("manifest input changed");
    await git(config, ["ls-files", "--error-unmatch", "--", entry.path]);
  }
  const selectedArguments =
    operation.parameters[selector(request.parameters)] ?? [];
  for (const candidate of [
    operation.executable,
    ...operation.arguments,
    ...selectedArguments,
  ]) {
    const absolute = isAbsolute(candidate)
      ? resolve(candidate)
      : resolve(root, candidate);
    const rel = relative(root, absolute);
    if (!rel || rel.startsWith("..") || isAbsolute(rel)) continue;
    try {
      const stat = await lstat(absolute);
      if (stat.isFile() && !paths.has(rel))
        throw new Error("executable input absent from manifest");
    } catch (error) {
      if (error instanceof Error && error.message.includes("absent"))
        throw error;
    }
  }
  const dirty = await git(config, [
    "status",
    "--porcelain=v1",
    "--untracked-files=all",
    "--",
    ...entries.map((entry) => entry.path),
  ]);
  if (dirty.trim()) throw new Error("manifest input is dirty or untracked");
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
  if ((await currentRevision(config)) !== request.gitSha)
    throw new Error("checkout revision mismatch");
  await verifyInputs(config, request);
  return await new Promise<{
    stdout: string;
    stderr: string;
    status: "succeeded" | "failed";
  }>((done, reject) => {
    const child = spawn(
      operation.executable,
      [...operation.arguments, ...staticArgs],
      {
        cwd: resolve(config.checkoutDirectory),
        shell: false,
        detached: true,
        env: {
          PATH: process.env.PATH ?? "",
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
  };
  return {
    payload,
    signature: signObject(payload, privateKeyFromEnv(config.hostPrivateKeyEnv)),
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
