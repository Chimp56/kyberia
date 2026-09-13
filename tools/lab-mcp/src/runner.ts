import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
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
  return await new Promise((done, reject) => {
    const child = spawn("git", ["rev-parse", "HEAD"], {
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
async function execute(config: LabRunnerConfig, request: JobRequest) {
  const operation = config.operations[request.suite];
  if (!operation) throw new Error("operation not allowlisted");
  const staticArgs = operation.parameters[selector(request.parameters)];
  if (!staticArgs) throw new Error("parameter combination not allowlisted");
  if (request.timeoutSeconds > config.limits.timeoutSeconds)
    throw new Error("timeout rejected");
  if ((await currentRevision(config)) !== request.gitSha)
    throw new Error("checkout revision mismatch");
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
        env: { PATH: process.env.PATH ?? "" },
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
    let stdout = Buffer.alloc(0),
      stderr = Buffer.alloc(0),
      size = 0,
      settled = false;
    const stop = () => {
      if (settled || child.pid === undefined) return;
      try {
        process.kill(-child.pid, "SIGTERM");
      } catch {}
      setTimeout(() => {
        if (!settled && child.pid !== undefined)
          try {
            process.kill(-child.pid, "SIGKILL");
          } catch {}
      }, 2000).unref();
    };
    const add = (which: "stdout" | "stderr", chunk: Buffer) => {
      size += chunk.length;
      if (size > config.limits.outputBytes) {
        stop();
        return;
      }
      if (which === "stdout") stdout = Buffer.concat([stdout, chunk]);
      else stderr = Buffer.concat([stderr, chunk]);
    };
    child.stdout?.on("data", (c: Buffer) => add("stdout", c));
    child.stderr?.on("data", (c: Buffer) => add("stderr", c));
    const timer = setTimeout(stop, request.timeoutSeconds * 1000);
    timer.unref();
    child.on("error", reject);
    child.on("close", (code) => {
      settled = true;
      clearTimeout(timer);
      if (size > config.limits.outputBytes)
        reject(new Error("command output exceeded limit"));
      else
        done({
          stdout: stdout.toString("utf8"),
          stderr: stderr.toString("utf8"),
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
  if (
    challenge.schemaVersion !== 1 ||
    challenge.hostId !== config.hostId ||
    !/^[A-Za-z0-9_-]{43}$/.test(challenge.nonce) ||
    !Number.isFinite(Date.parse(challenge.issuedAt))
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
