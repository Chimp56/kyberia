import { spawn } from "node:child_process";
import { randomBytes, randomUUID } from "node:crypto";
import { lstat, mkdir, readFile, writeFile } from "node:fs/promises";
import { isAbsolute, relative, resolve } from "node:path";
import { z } from "zod";
import type { LabConfig, ProbeId, SuiteId } from "./schema.js";
import { ARTIFACT, ID, SHA } from "./schema.js";
import {
  authenticateHostResponse,
  canonical,
  digest,
  privateKeyFromEnv,
  publicKeyFromEnv,
  sanitize,
  signObject,
  verifyObject,
} from "./security.js";

type Status =
  | "queued"
  | "running"
  | "cancelling"
  | "succeeded"
  | "failed"
  | "cancelled";
type Spec = {
  executable: string;
  arguments: string[];
  version: string;
  environment: { KYBERIA_LAB_RUNNER_CONFIG: string };
};
type Host = LabConfig["hosts"][number];
export interface JobRequest {
  schemaVersion: 1;
  runId: string;
  hostId: string;
  gitSha: string;
  suite: string;
  seed: number;
  timeoutSeconds: number;
  parameters: Record<string, string>;
  nonce: string;
  issuedAt: string;
  coordinatorKeyId: string;
  specVersion: string;
}
export interface SignedJobRequest {
  request: JobRequest;
  signature: string;
  algorithm: "Ed25519";
}
export interface HostChallenge {
  schemaVersion: 1;
  hostId: string;
  nonce: string;
  issuedAt: string;
}
const RunnerPayloadSchema = z
  .object({
    schemaVersion: z.literal(1),
    requestDigest: z.string().regex(/^sha256:[0-9a-f]{64}$/),
    hostId: ID,
    status: z.enum(["succeeded", "failed", "cancelled"]),
    startedAt: z.string().datetime(),
    finishedAt: z.string().datetime(),
    stdout: z.string(),
    stderr: z.string(),
    capabilities: z.array(ID).max(64),
  })
  .strict();
export type RunnerPayload = z.infer<typeof RunnerPayloadSchema>;
const RunnerResponseSchema = z
  .object({
    payload: RunnerPayloadSchema,
    signature: z.string().base64().max(256),
    algorithm: z.literal("Ed25519"),
  })
  .strict();
export interface Artifact {
  name: string;
  mediaType: "text/plain";
  bytes: number;
  sha256: string;
  sanitization: string;
  truncated: boolean;
}
export interface Manifest {
  schemaVersion: 1;
  runId: string;
  hostId: string;
  hostIdentity: string;
  capabilities: string[];
  gitSha: string;
  suite: string;
  seed: number;
  createdAt: string;
  startedAt: string;
  finishedAt: string;
  status: Status;
  specVersion: string;
  requestDigest: string;
  hostResultSignature: string;
  artifacts: Artifact[];
}
interface StoredRun {
  id: string;
  status: Status;
  hostId: string;
  suite: string;
  gitSha: string;
  seed: number;
  createdAt: string;
  manifest?: Manifest;
  signature?: string;
  error?: string;
  controller: AbortController;
  queued?: () => void;
}
export interface Executor {
  authenticate(
    spec: Spec,
    challenge: HostChallenge,
    timeoutMs: number,
    maxOutput: number,
  ): Promise<string>;
  execute(
    spec: Spec,
    request: SignedJobRequest,
    timeoutMs: number,
    signal: AbortSignal,
    maxOutput: number,
  ): Promise<string>;
}

export class ProcessExecutor implements Executor {
  async authenticate(
    spec: Spec,
    challenge: HostChallenge,
    timeoutMs: number,
    maxOutput: number,
  ) {
    return this.exchange(
      spec,
      canonical({ kind: "authenticate", challenge }),
      timeoutMs,
      new AbortController().signal,
      maxOutput,
    );
  }
  async execute(
    spec: Spec,
    request: SignedJobRequest,
    timeoutMs: number,
    signal: AbortSignal,
    maxOutput: number,
  ): Promise<string> {
    return this.exchange(
      spec,
      canonical(request),
      timeoutMs,
      signal,
      maxOutput,
    );
  }
  private async exchange(
    spec: Spec,
    input: string,
    timeoutMs: number,
    signal: AbortSignal,
    maxOutput: number,
  ): Promise<string> {
    return await new Promise<string>((resolvePromise, reject) => {
      const child = spawn(spec.executable, spec.arguments, {
        shell: false,
        detached: process.platform !== "win32",
        windowsHide: true,
        stdio: ["pipe", "pipe", "pipe"],
        env: {
          PATH: process.env.PATH ?? "",
          KYBERIA_LAB_RUNNER_CONFIG: spec.environment.KYBERIA_LAB_RUNNER_CONFIG,
        },
      });
      if (!child.stdin || !child.stdout || !child.stderr) {
        reject(new Error("runner pipes unavailable"));
        return;
      }
      let response = Buffer.alloc(0),
        stderrBytes = 0,
        settled = false;
      const stop = () => {
        if (settled || child.pid === undefined) return;
        try {
          process.kill(
            process.platform === "win32" ? child.pid : -child.pid,
            "SIGTERM",
          );
        } catch {}
        setTimeout(() => {
          if (!settled && child.pid !== undefined)
            try {
              process.kill(
                process.platform === "win32" ? child.pid : -child.pid,
                "SIGKILL",
              );
            } catch {}
        }, 2000).unref();
      };
      child.stdout.on("data", (chunk: Buffer) => {
        response = Buffer.concat([response, chunk]);
        if (response.length > maxOutput) stop();
      });
      child.stderr.on("data", (chunk: Buffer) => {
        stderrBytes += chunk.length;
        if (stderrBytes > maxOutput) stop();
      });
      signal.addEventListener("abort", stop, { once: true });
      const timer = setTimeout(stop, timeoutMs);
      timer.unref();
      child.once("error", reject);
      child.once("close", (code) => {
        settled = true;
        clearTimeout(timer);
        signal.removeEventListener("abort", stop);
        if (response.length > maxOutput || stderrBytes > maxOutput)
          reject(new Error("runner response exceeded output limit"));
        else if (code !== 0) reject(new Error("authenticated runner failed"));
        else resolvePromise(response.toString("utf8"));
      });
      child.stdin.end(`${input}\n`);
    });
  }
}

export class LabManager {
  readonly runs = new Map<string, StoredRun>();
  private active = 0;
  private queue: StoredRun[] = [];
  constructor(
    readonly config: LabConfig,
    private readonly executor: Executor = new ProcessExecutor(),
    private readonly now = () => new Date(),
  ) {}
  listHosts() {
    return this.config.hosts.map(
      ({ id, displayName, identity, capabilities, suites, probes }) => ({
        id,
        displayName,
        identity,
        capabilities,
        suites: Object.keys(suites),
        probes: Object.keys(probes),
      }),
    );
  }
  capabilities(id: string) {
    return this.host(id).capabilities;
  }
  private host(id: string): Host {
    ID.parse(id);
    const host = this.config.hosts.find((item) => item.id === id);
    if (!host) throw new Error("unknown host");
    publicKeyFromEnv(host.publicKeyEnv);
    return host;
  }
  private admitSha(sha: string) {
    SHA.parse(sha);
    if (!this.config.immutableRevisions.includes(sha))
      throw new Error("revision is not admitted by immutable-revision policy");
  }
  async submit(
    hostId: string,
    sha: string,
    suite: SuiteId,
    seed: number,
    timeout: number,
  ) {
    this.admitSha(sha);
    return this.enqueue(
      hostId,
      sha,
      suite,
      seed,
      timeout,
      {},
      this.host(hostId).suites[suite],
    );
  }
  async probe(
    hostId: string,
    probe: ProbeId,
    parameters: Record<string, string> = {},
  ) {
    const host = this.host(hostId);
    if (
      probe === "kismet" &&
      parameters.expected_version &&
      !host.allowedKismetVersions.includes(parameters.expected_version)
    )
      throw new Error("Kismet version is not allowlisted");
    if (
      parameters.fixture_set &&
      !host.allowedFixtureSets.includes(parameters.fixture_set)
    )
      throw new Error("fixture set is not allowlisted");
    if (
      parameters.scene_set &&
      !host.allowedSceneSets.includes(parameters.scene_set)
    )
      throw new Error("scene set is not allowlisted");
    return this.enqueue(
      hostId,
      this.config.immutableRevisions[0]!,
      `probe-${probe}`,
      0,
      this.config.limits.timeoutSeconds,
      parameters,
      host.probes[probe],
    );
  }
  private async enqueue(
    hostId: string,
    sha: string,
    suite: string,
    seed: number,
    timeout: number,
    parameters: Record<string, string>,
    spec?: Spec,
  ) {
    const host = this.host(hostId);
    if (!spec) throw new Error("operation unsupported by host");
    if (timeout > this.config.limits.timeoutSeconds)
      throw new Error("timeout exceeds operator limit");
    if (
      this.active + this.queue.length >=
      this.config.limits.concurrency + this.config.limits.queue
    )
      throw new Error("lab queue is full");
    await this.authenticateBeforeWork(host, spec);
    const id = `run-${randomUUID()}`;
    const run: StoredRun = {
      id,
      status: "queued",
      hostId,
      suite,
      gitSha: sha,
      seed,
      createdAt: this.now().toISOString(),
      controller: new AbortController(),
    };
    this.runs.set(id, run);
    const request: JobRequest = {
      schemaVersion: 1,
      runId: id,
      hostId,
      gitSha: sha,
      suite,
      seed,
      timeoutSeconds: timeout,
      parameters,
      nonce: randomBytes(32).toString("base64url"),
      issuedAt: this.now().toISOString(),
      coordinatorKeyId: this.config.coordinatorKeyId,
      specVersion: spec.version,
    };
    const signed: SignedJobRequest = {
      request,
      signature: signObject(
        request,
        privateKeyFromEnv(this.config.manifestPrivateKeyEnv),
      ),
      algorithm: "Ed25519",
    };
    run.queued = () => void this.perform(run, host, spec, signed, timeout);
    this.queue.push(run);
    this.pump();
    return id;
  }
  private async authenticateBeforeWork(host: Host, spec: Spec) {
    const challenge: HostChallenge = {
      schemaVersion: 1,
      hostId: host.id,
      nonce: randomBytes(32).toString("base64url"),
      issuedAt: this.now().toISOString(),
    };
    const raw = await this.executor.authenticate(spec, challenge, 5000, 4096);
    const proof = z
      .object({
        payload: z
          .object({
            schemaVersion: z.literal(1),
            hostId: ID,
            challengeDigest: z.string().regex(/^sha256:[0-9a-f]{64}$/),
            capabilities: z.array(ID).max(64),
          })
          .strict(),
        signature: z.string().base64().max(256),
        algorithm: z.literal("Ed25519"),
      })
      .strict()
      .parse(JSON.parse(raw));
    const key = publicKeyFromEnv(host.publicKeyEnv);
    authenticateHostResponse(
      proof.payload,
      proof.signature,
      key,
      host.identity,
    );
    if (
      proof.payload.hostId !== host.id ||
      proof.payload.challengeDigest !== digest(canonical(challenge)) ||
      proof.payload.capabilities.some(
        (capability) => !host.capabilities.includes(capability),
      )
    )
      throw new Error("host preflight binding failed");
  }
  status(id: string) {
    const run = this.runs.get(ID.parse(id));
    if (!run) throw new Error("unknown run");
    return {
      runId: run.id,
      status: run.status,
      hostId: run.hostId,
      suite: run.suite,
      gitSha: run.gitSha,
      error: run.error,
    };
  }
  cancel(id: string) {
    const run = this.runs.get(ID.parse(id));
    if (!run) throw new Error("unknown run");
    if (run.status === "queued") {
      this.queue = this.queue.filter((candidate) => candidate !== run);
      run.status = "cancelling";
      void this.finalizeCancelled(run);
    } else if (run.status === "running") {
      run.status = "cancelling";
      run.controller.abort();
    }
    return this.status(id);
  }
  manifest(id: string) {
    const run = this.runs.get(ID.parse(id));
    if (!run?.manifest || !run.signature)
      throw new Error("manifest unavailable until run completion");
    return {
      manifest: run.manifest,
      signature: run.signature,
      algorithm: "Ed25519",
      keyId: this.config.coordinatorKeyId,
    };
  }
  verifyManifest(id: string, expectedHost?: string) {
    const signed = this.manifest(id);
    return (
      signed.manifest.runId === id &&
      (!expectedHost || signed.manifest.hostId === expectedHost) &&
      verifyObject(
        signed.manifest,
        signed.signature,
        publicKeyFromEnv(this.config.manifestPublicKeyEnv),
      )
    );
  }
  artifacts(id: string) {
    return this.manifest(id).manifest.artifacts;
  }
  async fetch(id: string, name: string) {
    ARTIFACT.parse(name);
    const artifact = this.artifacts(id).find((item) => item.name === name);
    if (!artifact) throw new Error("artifact unavailable");
    const path = this.artifactPath(id, name);
    const stat = await lstat(path);
    if (!stat.isFile() || stat.isSymbolicLink())
      throw new Error("artifact type rejected");
    const data = await readFile(path);
    if (digest(data) !== artifact.sha256)
      throw new Error("artifact integrity check failed");
    return { ...artifact, content: data.toString("utf8") };
  }
  private artifactPath(id: string, name: string) {
    ID.parse(id);
    ARTIFACT.parse(name);
    const root = resolve(this.config.stateDirectory, id);
    const path = resolve(root, name);
    const rel = relative(root, path);
    if (!rel || rel.startsWith("..") || isAbsolute(rel))
      throw new Error("artifact path rejected");
    return path;
  }
  private pump() {
    while (this.active < this.config.limits.concurrency && this.queue.length) {
      const run = this.queue.shift()!;
      if (run.status !== "queued") continue;
      this.active++;
      run.queued!();
    }
  }
  private async finalizeCancelled(run: StoredRun) {
    const timestamp = this.now().toISOString();
    const host = this.host(run.hostId);
    await this.writeManifest(
      run,
      host,
      {
        schemaVersion: 1,
        requestDigest: digest("queued-cancelled"),
        hostId: run.hostId,
        status: "cancelled",
        startedAt: timestamp,
        finishedAt: timestamp,
        stdout: "",
        stderr: "",
        capabilities: host.capabilities,
      },
      "coordinator-cancelled-before-dispatch",
      "cancelled",
    );
    run.status = "cancelled";
  }
  private async perform(
    run: StoredRun,
    host: Host,
    spec: Spec,
    signed: SignedJobRequest,
    timeout: number,
  ) {
    if (run.status === "cancelling") {
      this.active--;
      this.pump();
      return;
    }
    run.status = "running";
    try {
      const raw = await this.executor.execute(
        spec,
        signed,
        timeout * 1000,
        run.controller.signal,
        this.config.limits.outputBytes,
      );
      const response = RunnerResponseSchema.parse(JSON.parse(raw));
      const hostKey = publicKeyFromEnv(host.publicKeyEnv);
      authenticateHostResponse(
        response.payload,
        response.signature,
        hostKey,
        host.identity,
      );
      if (
        response.payload.requestDigest !== digest(canonical(signed)) ||
        response.payload.hostId !== host.id
      )
        throw new Error("host response does not bind the submitted request");
      if (
        response.payload.capabilities.some(
          (capability) => !host.capabilities.includes(capability),
        )
      )
        throw new Error("host asserted an unpinned capability");
      const finalStatus = run.controller.signal.aborted
        ? "cancelled"
        : response.payload.status;
      await this.writeManifest(
        run,
        host,
        response.payload,
        response.signature,
        finalStatus,
      );
      run.status = finalStatus;
    } catch (_error) {
      const finalStatus = run.controller.signal.aborted
        ? "cancelled"
        : "failed";
      run.error = "runner execution or authentication failed";
      const timestamp = this.now().toISOString();
      await this.writeManifest(
        run,
        host,
        {
          schemaVersion: 1,
          requestDigest: digest(canonical(signed)),
          hostId: host.id,
          status: finalStatus,
          startedAt: timestamp,
          finishedAt: timestamp,
          stdout: "",
          stderr: run.error,
          capabilities: host.capabilities,
        },
        "coordinator-failure",
        finalStatus,
      );
      run.status = finalStatus;
    } finally {
      this.active--;
      this.pump();
    }
  }
  private specVersion(run: StoredRun, host: Host) {
    return (
      host.suites[run.suite as SuiteId]?.version ??
      host.probes[run.suite.replace("probe-", "") as ProbeId]?.version ??
      "coordinator-v1"
    );
  }
  private async writeManifest(
    run: StoredRun,
    host: Host,
    payload: RunnerPayload,
    hostSignature: string,
    status: Status,
  ) {
    const dir = resolve(this.config.stateDirectory, run.id);
    await mkdir(dir, { recursive: true, mode: 0o700 });
    const artifacts: Artifact[] = [];
    for (const [name, raw] of [
      ["stdout.txt", payload.stdout],
      ["stderr.txt", payload.stderr],
    ] as const) {
      if (artifacts.length >= this.config.limits.artifactCount) break;
      const cleaned = sanitize(
        raw,
        Math.min(
          this.config.limits.outputBytes,
          this.config.limits.artifactBytes,
        ),
      );
      const bytes = Buffer.from(cleaned.text);
      await writeFile(this.artifactPath(run.id, name), bytes, {
        mode: 0o600,
        flag: "wx",
      });
      artifacts.push({
        name,
        mediaType: "text/plain",
        bytes: bytes.length,
        sha256: digest(bytes),
        sanitization: cleaned.policy,
        truncated: cleaned.truncated,
      });
    }
    const manifest: Manifest = {
      schemaVersion: 1,
      runId: run.id,
      hostId: host.id,
      hostIdentity: host.identity,
      capabilities: [...payload.capabilities].sort(),
      gitSha: run.gitSha,
      suite: run.suite,
      seed: run.seed,
      createdAt: run.createdAt,
      startedAt: payload.startedAt,
      finishedAt: payload.finishedAt,
      status,
      specVersion: this.specVersion(run, host),
      requestDigest: payload.requestDigest,
      hostResultSignature: hostSignature,
      artifacts,
    };
    if (
      Buffer.byteLength(canonical(manifest)) > this.config.limits.manifestBytes
    )
      throw new Error("manifest size exceeds operator limit");
    run.manifest = manifest;
    run.signature = signObject(
      manifest,
      privateKeyFromEnv(this.config.manifestPrivateKeyEnv),
    );
    await writeFile(
      resolve(dir, "manifest.json"),
      canonical({ manifest, signature: run.signature, algorithm: "Ed25519" }),
      { mode: 0o600, flag: "wx" },
    );
  }
}
