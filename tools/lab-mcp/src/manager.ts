import { spawn } from "node:child_process";
import { createPublicKey, randomBytes, randomUUID } from "node:crypto";
import { lstat, mkdir, readFile, rename, writeFile } from "node:fs/promises";
import {
  existsSync,
  readdirSync,
  readFileSync,
  renameSync,
  writeFileSync,
} from "node:fs";
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
  publicKeyIdentity,
  sanitize,
  signObject,
  verifyExecutable,
  verifyInvocationArguments,
  verifyObject,
} from "./security.js";

type Status =
  | "queued"
  | "running"
  | "cancelling"
  | "succeeded"
  | "failed"
  | "cancelled";
const StatusSchema = z.enum([
  "queued",
  "running",
  "cancelling",
  "succeeded",
  "failed",
  "cancelled",
]);
type Spec = {
  executable: string;
  executableSha256: string;
  arguments: string[];
  argumentFiles: Array<{ argumentIndex: number; sha256: string }>;
  version: string;
  environment: { KYBERIA_LAB_RUNNER_CONFIG: string };
  credentialEnvNames: string[];
  inputManifestId: string;
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
  inputManifestId: string;
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
    sanitization: z.literal("kyberia-lab-text-v2"),
    capabilities: z.array(ID).max(64),
    toolIdentities: z
      .array(
        z
          .object({
            role: z.enum(["git", "operation"]),
            id: ID,
            version: z.string().min(1).max(64),
            sha256: z.string().regex(/^[0-9a-f]{64}$/),
          })
          .strict(),
      )
      .max(2),
  })
  .strict();
export type RunnerPayload = z.infer<typeof RunnerPayloadSchema>;
const RunnerResponseSchema = z
  .object({
    payload: RunnerPayloadSchema,
    signedPreimage: z
      .object({
        schemaVersion: z.literal(1),
        payloadDigest: z.string().regex(/^sha256:[0-9a-f]{64}$/),
      })
      .strict(),
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
  inputManifestId: string;
  requestDigest: string;
  toolIdentities: Array<{
    role: "git" | "operation";
    id: string;
    version: string;
    sha256: string;
  }>;
  evidence:
    | {
        origin: "host-signed";
        signedPreimage: { schemaVersion: 1; payloadDigest: string };
        hostSignature: string;
        hostPayload: RunnerPayload;
      }
    | { origin: "coordinator-terminal"; reason: string };
  artifacts: Artifact[];
}
const ArtifactSchema = z
  .object({
    name: ARTIFACT,
    mediaType: z.literal("text/plain"),
    bytes: z.number().int().min(0),
    sha256: z.string().regex(/^sha256:[0-9a-f]{64}$/),
    sanitization: ID,
    truncated: z.boolean(),
  })
  .strict();
const ManifestSchema: z.ZodType<Manifest> = z
  .object({
    schemaVersion: z.literal(1),
    runId: ID,
    hostId: ID,
    hostIdentity: z.string().regex(/^sha256:[A-Za-z0-9+/]{43}=$/),
    capabilities: z.array(ID).max(64),
    gitSha: SHA,
    suite: ID,
    seed: z.number().int().min(0).max(0xffff_ffff),
    createdAt: z.string().datetime(),
    startedAt: z.string().datetime(),
    finishedAt: z.string().datetime(),
    status: StatusSchema,
    specVersion: z.string().min(1).max(64),
    inputManifestId: z.string().regex(/^sha256:[0-9a-f]{64}$/),
    requestDigest: z.string().regex(/^sha256:[0-9a-f]{64}$/),
    toolIdentities: RunnerPayloadSchema.shape.toolIdentities,
    evidence: z.discriminatedUnion("origin", [
      z
        .object({
          origin: z.literal("host-signed"),
          signedPreimage: z
            .object({
              schemaVersion: z.literal(1),
              payloadDigest: z.string().regex(/^sha256:[0-9a-f]{64}$/),
            })
            .strict(),
          hostSignature: z.string().base64().max(256),
          hostPayload: RunnerPayloadSchema,
        })
        .strict(),
      z
        .object({
          origin: z.literal("coordinator-terminal"),
          reason: ID,
        })
        .strict(),
    ]),
    artifacts: z.array(ArtifactSchema).max(32),
  })
  .strict();
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
type PersistedRun = Omit<StoredRun, "controller" | "queued" | "manifest"> & {
  manifest?: Manifest;
};
const PersistedRunSchema = z
  .object({
    id: ID,
    status: StatusSchema,
    hostId: ID,
    suite: ID,
    gitSha: SHA,
    seed: z.number().int().min(0).max(0xffff_ffff),
    createdAt: z.string().datetime(),
    manifest: ManifestSchema.optional(),
    signature: z.string().base64().max(256).optional(),
    error: z.string().max(512).optional(),
  })
  .strict();
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
    await verifyExecutable(spec.executable, spec.executableSha256);
    await verifyInvocationArguments(spec.arguments, spec.argumentFiles);
    return await new Promise<string>((resolvePromise, reject) => {
      const child = spawn(spec.executable, spec.arguments, {
        shell: false,
        detached: process.platform !== "win32",
        windowsHide: true,
        stdio: ["pipe", "pipe", "pipe"],
        env: {
          PATH: "",
          KYBERIA_LAB_RUNNER_CONFIG: spec.environment.KYBERIA_LAB_RUNNER_CONFIG,
          ...Object.fromEntries(
            spec.credentialEnvNames.map((name) => {
              const value = process.env[name];
              if (!value)
                throw new Error("allowlisted runner credential unavailable");
              return [name, value];
            }),
          ),
        },
      });
      if (!child.stdin || !child.stdout || !child.stderr) {
        reject(new Error("runner pipes unavailable"));
        return;
      }
      const response: Buffer[] = [];
      let responseBytes = 0,
        stderrBytes = 0,
        settled = false,
        stopping = false;
      const finish = (error?: Error, value?: string) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        clearTimeout(hardTimer);
        signal.removeEventListener("abort", stop);
        error ? reject(error) : resolvePromise(value ?? "");
      };
      let hardTimer: NodeJS.Timeout | undefined;
      const stop = () => {
        if (settled || stopping) return;
        stopping = true;
        try {
          if (child.pid !== undefined)
            process.kill(
              process.platform === "win32" ? child.pid : -child.pid,
              "SIGTERM",
            );
        } catch {}
        hardTimer = setTimeout(() => {
          if (!settled && child.pid !== undefined)
            try {
              process.kill(
                process.platform === "win32" ? child.pid : -child.pid,
                "SIGKILL",
              );
            } catch {}
          setTimeout(
            () =>
              finish(new Error("runner did not terminate after cancellation")),
            500,
          ).unref();
        }, 2000);
        hardTimer.unref();
      };
      child.stdout.on("data", (chunk: Buffer) => {
        responseBytes += chunk.length;
        if (responseBytes <= maxOutput) response.push(chunk);
        else stop();
      });
      child.stderr.on("data", (chunk: Buffer) => {
        stderrBytes += chunk.length;
        if (stderrBytes > maxOutput) stop();
      });
      signal.addEventListener("abort", stop, { once: true });
      const timer = setTimeout(stop, timeoutMs);
      timer.unref();
      child.once("error", (error) => finish(error));
      child.once("close", (code) => {
        if (responseBytes > maxOutput || stderrBytes > maxOutput)
          finish(new Error("runner response exceeded output limit"));
        else if (signal.aborted) finish(new Error("runner cancelled"));
        else if (code !== 0) finish(new Error("authenticated runner failed"));
        else finish(undefined, Buffer.concat(response).toString("utf8"));
      });
      child.stdin.end(`${input}\n`);
    });
  }
}

export class LabManager {
  readonly runs = new Map<string, StoredRun>();
  private active = 0;
  private reservations = 0;
  private queue: StoredRun[] = [];
  constructor(
    readonly config: LabConfig,
    private readonly executor: Executor = new ProcessExecutor(),
    private readonly now = () => new Date(),
  ) {
    this.validateKeyRoles();
    this.recover();
  }
  private validateKeyRoles() {
    const coordinatorPrivate = privateKeyFromEnv(
      this.config.manifestPrivateKeyEnv,
    );
    const coordinatorPublic = publicKeyFromEnv(
      this.config.manifestPublicKeyEnv,
    );
    const coordinatorIdentity = publicKeyIdentity(coordinatorPublic);
    if (
      publicKeyIdentity(createPublicKey(coordinatorPrivate)) !==
      coordinatorIdentity
    )
      throw new Error("coordinator signing key pair mismatch");
    const hostIds = new Set<string>();
    const hostIdentities = new Set<string>();
    for (const host of this.config.hosts) {
      const identity = publicKeyIdentity(publicKeyFromEnv(host.publicKeyEnv));
      if (
        hostIds.has(host.id) ||
        hostIdentities.has(identity) ||
        identity === coordinatorIdentity ||
        identity !== host.identity
      )
        throw new Error("coordinator and host key identities must be distinct");
      hostIds.add(host.id);
      hostIdentities.add(identity);
    }
  }
  private recover() {
    const root = resolve(this.config.stateDirectory);
    if (!existsSync(root)) return;
    for (const entry of readdirSync(root, { withFileTypes: true })) {
      if (!entry.isDirectory() || !ID.safeParse(entry.name).success) continue;
      const statePath = resolve(root, entry.name, "status.json");
      if (!existsSync(statePath)) continue;
      try {
        const raw = PersistedRunSchema.parse(
          JSON.parse(readFileSync(statePath, "utf8")),
        );
        if (raw.id !== entry.name)
          throw new Error("run state identity mismatch");
        if (
          (raw.manifest || raw.signature) &&
          (!raw.manifest ||
            !raw.signature ||
            raw.manifest.runId !== raw.id ||
            !verifyObject(
              raw.manifest,
              raw.signature,
              publicKeyFromEnv(this.config.manifestPublicKeyEnv),
            ) ||
            !this.verifyEvidence(raw.manifest))
        )
          throw new Error("persisted manifest signature invalid");
        const status: Status = ["queued", "running", "cancelling"].includes(
          raw.status,
        )
          ? "failed"
          : raw.status;
        const run: StoredRun = {
          id: raw.id,
          status,
          hostId: raw.hostId,
          suite: raw.suite,
          gitSha: raw.gitSha,
          seed: raw.seed,
          createdAt: raw.createdAt,
          ...(raw.manifest ? { manifest: raw.manifest } : {}),
          ...(raw.signature ? { signature: raw.signature } : {}),
          ...(status === "failed" && raw.status !== "failed"
            ? { error: "coordinator restarted before run completion" }
            : raw.error
              ? { error: raw.error }
              : {}),
          controller: new AbortController(),
        };
        if (status === "failed" && raw.status !== "failed") {
          const host = this.config.hosts.find((item) => item.id === raw.hostId);
          if (host) {
            const timestamp = this.now().toISOString();
            run.manifest = {
              schemaVersion: 1,
              runId: run.id,
              hostId: run.hostId,
              hostIdentity: host.identity,
              capabilities: [...host.capabilities].sort(),
              gitSha: run.gitSha,
              suite: run.suite,
              seed: run.seed,
              createdAt: run.createdAt,
              startedAt: timestamp,
              finishedAt: timestamp,
              status: "failed",
              specVersion: this.specVersion(run, host),
              inputManifestId: this.inputManifestId(run, host),
              requestDigest: digest(`recovered:${run.id}`),
              toolIdentities: [],
              evidence: {
                origin: "coordinator-terminal",
                reason: "restart-recovery",
              },
              artifacts: [],
            };
            run.signature = signObject(
              run.manifest,
              privateKeyFromEnv(this.config.manifestPrivateKeyEnv),
            );
            this.persistSync(run);
          }
        }
        this.runs.set(entry.name, run);
      } catch {
        // A malformed status record is ignored rather than trusted.
      }
    }
  }
  private record(run: StoredRun): PersistedRun {
    return {
      id: run.id,
      status: run.status,
      hostId: run.hostId,
      suite: run.suite,
      gitSha: run.gitSha,
      seed: run.seed,
      createdAt: run.createdAt,
      ...(run.manifest ? { manifest: run.manifest } : {}),
      ...(run.signature ? { signature: run.signature } : {}),
      ...(run.error ? { error: run.error } : {}),
    };
  }
  private persistSync(run: StoredRun) {
    const dir = resolve(this.config.stateDirectory, run.id);
    if (run.manifest && run.signature) {
      const manifestTarget = resolve(dir, "manifest.json");
      const manifestTemporary = resolve(dir, `manifest-${randomUUID()}.tmp`);
      writeFileSync(
        manifestTemporary,
        canonical({
          manifest: run.manifest,
          signature: run.signature,
          algorithm: "Ed25519",
        }),
        { mode: 0o600, flag: "wx" },
      );
      renameSync(manifestTemporary, manifestTarget);
    }
    const target = resolve(dir, "status.json");
    const temporary = resolve(dir, `status-${randomUUID()}.tmp`);
    writeFileSync(temporary, canonical(this.record(run)), {
      mode: 0o600,
      flag: "wx",
    });
    renameSync(temporary, target);
  }
  private async persist(run: StoredRun) {
    const dir = resolve(this.config.stateDirectory, run.id);
    await mkdir(dir, { recursive: true, mode: 0o700 });
    const target = resolve(dir, "status.json");
    const temporary = resolve(dir, `status-${randomUUID()}.tmp`);
    await writeFile(temporary, canonical(this.record(run)), {
      mode: 0o600,
      flag: "wx",
    });
    await rename(temporary, target);
  }
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
      this.active + this.queue.length + this.reservations >=
      this.config.limits.concurrency + this.config.limits.queue
    )
      throw new Error("lab queue is full");
    if (this.reservations >= this.config.limits.concurrency)
      throw new Error("host authentication capacity is full");
    this.reservations++;
    try {
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
        inputManifestId: spec.inputManifestId,
      };
      const signed: SignedJobRequest = {
        request,
        signature: signObject(
          request,
          privateKeyFromEnv(this.config.manifestPrivateKeyEnv),
        ),
        algorithm: "Ed25519",
      };
      await this.persist(run);
      this.runs.set(id, run);
      run.queued = () => void this.perform(run, host, spec, signed, timeout);
      this.queue.push(run);
      this.pump();
      return id;
    } finally {
      this.reservations--;
    }
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
    const issuedAt = Date.parse(challenge.issuedAt);
    if (
      !Number.isFinite(issuedAt) ||
      Math.abs(this.now().getTime() - issuedAt) > 10_000
    )
      throw new Error("host challenge freshness failed");
    if (
      proof.payload.hostId !== host.id ||
      proof.payload.challengeDigest !== digest(canonical(challenge)) ||
      canonical([...proof.payload.capabilities].sort()) !==
        canonical([...host.capabilities].sort())
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
      void this.persist(run)
        .then(() => this.finalizeCancelled(run))
        .catch(() => {
          run.status = "failed";
          run.error = "cancelled run evidence publication failed";
          void this.persist(run).catch(() => undefined);
        });
    } else if (run.status === "running") {
      run.status = "cancelling";
      void this.persist(run).catch(() => undefined);
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
      this.verifyEvidence(signed.manifest) &&
      verifyObject(
        signed.manifest,
        signed.signature,
        publicKeyFromEnv(this.config.manifestPublicKeyEnv),
      )
    );
  }
  private verifyEvidence(manifest: Manifest): boolean {
    try {
      if (manifest.evidence.origin === "coordinator-terminal")
        return manifest.toolIdentities.length === 0;
      const payload = manifest.evidence.hostPayload;
      const host = this.config.hosts.find(
        (item) => item.id === manifest.hostId,
      );
      if (!host) return false;
      if (
        manifest.evidence.signedPreimage.payloadDigest !==
          digest(canonical(payload)) ||
        !verifyObject(
          manifest.evidence.signedPreimage,
          manifest.evidence.hostSignature,
          publicKeyFromEnv(host.publicKeyEnv),
        ) ||
        payload.hostId !== manifest.hostId ||
        payload.requestDigest !== manifest.requestDigest ||
        payload.startedAt !== manifest.startedAt ||
        payload.finishedAt !== manifest.finishedAt ||
        payload.status !== manifest.status ||
        canonical([...payload.capabilities].sort()) !==
          canonical(manifest.capabilities) ||
        canonical(payload.toolIdentities) !== canonical(manifest.toolIdentities)
      )
        return false;
      if (
        sanitize(payload.stdout, this.config.limits.outputBytes).text !==
          payload.stdout ||
        sanitize(payload.stderr, this.config.limits.outputBytes).text !==
          payload.stderr
      )
        return false;
      const outputs = [
        ["stdout.txt", payload.stdout],
        ["stderr.txt", payload.stderr],
      ] as const;
      const expectedOutputs = outputs.slice(
        0,
        this.config.limits.artifactCount,
      );
      if (manifest.artifacts.length !== expectedOutputs.length) return false;
      for (const [name, raw] of expectedOutputs) {
        const metadata = manifest.artifacts.find((item) => item.name === name);
        if (!metadata) return false;
        const cleaned = sanitize(
          raw,
          Math.min(
            this.config.limits.outputBytes,
            this.config.limits.artifactBytes,
          ),
        );
        const bytes = Buffer.from(cleaned.text);
        if (
          metadata.sha256 !== digest(bytes) ||
          metadata.bytes !== bytes.length ||
          metadata.sanitization !== cleaned.policy ||
          metadata.truncated !== cleaned.truncated ||
          digest(readFileSync(this.artifactPath(manifest.runId, name))) !==
            metadata.sha256
        )
          return false;
      }
      return true;
    } catch {
      return false;
    }
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
      run.status = "running";
      void this.persist(run)
        .then(() => run.queued!())
        .catch(() => {
          run.status = "failed";
          run.error = "run intent persistence failed";
          this.active--;
          this.pump();
        });
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
        sanitization: "kyberia-lab-text-v2",
        capabilities: host.capabilities,
        toolIdentities: [],
      },
      { origin: "coordinator-terminal", reason: "cancelled-before-dispatch" },
      "cancelled",
    );
    run.status = "cancelled";
    await this.persist(run);
  }
  private async perform(
    run: StoredRun,
    host: Host,
    spec: Spec,
    signed: SignedJobRequest,
    timeout: number,
  ) {
    if (run.status === "cancelling") {
      try {
        await this.finalizeCancelled(run);
      } catch {
        run.status = "failed";
        run.error = "cancelled run evidence publication failed";
        await this.persist(run).catch(() => undefined);
      } finally {
        this.active--;
        this.pump();
      }
      return;
    }
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
        response.signedPreimage,
        response.signature,
        hostKey,
        host.identity,
      );
      if (
        response.signedPreimage.payloadDigest !==
          digest(canonical(response.payload)) ||
        response.payload.requestDigest !== digest(canonical(signed)) ||
        response.payload.hostId !== host.id
      )
        throw new Error("host response does not bind the submitted request");
      if (
        canonical([...response.payload.capabilities].sort()) !==
        canonical([...host.capabilities].sort())
      )
        throw new Error("host asserted an unpinned capability");
      if (
        response.payload.toolIdentities.length !== 2 ||
        new Set(response.payload.toolIdentities.map(({ role }) => role))
          .size !== 2
      )
        throw new Error("host tool identity evidence is incomplete");
      const issuedAt = Date.parse(signed.request.issuedAt);
      const startedAt = Date.parse(response.payload.startedAt);
      const finishedAt = Date.parse(response.payload.finishedAt);
      if (
        startedAt < issuedAt - 5_000 ||
        finishedAt < startedAt ||
        finishedAt - startedAt > timeout * 1000 + 3_000 ||
        finishedAt > this.now().getTime() + 5_000
      )
        throw new Error("host result timing rejected");
      const finalStatus = response.payload.status;
      await this.writeManifest(
        run,
        host,
        response.payload,
        {
          origin: "host-signed",
          signedPreimage: response.signedPreimage,
          hostSignature: response.signature,
          hostPayload: response.payload,
        },
        finalStatus,
      );
      run.status = finalStatus;
      await this.persist(run);
    } catch (_error) {
      const finalStatus = run.controller.signal.aborted
        ? "cancelled"
        : "failed";
      run.error = "runner execution or authentication failed";
      const timestamp = this.now().toISOString();
      try {
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
            sanitization: "kyberia-lab-text-v2",
            capabilities: host.capabilities,
            toolIdentities: [],
          },
          { origin: "coordinator-terminal", reason: "runner-failure" },
          finalStatus,
        );
        run.status = finalStatus;
      } catch {
        run.status = "failed";
        run.error =
          "terminal evidence publication failed; retained status is recoverable";
        delete run.manifest;
        delete run.signature;
      }
      await this.persist(run).catch(() => undefined);
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
  private inputManifestId(run: StoredRun, host: Host) {
    return (
      host.suites[run.suite as SuiteId]?.inputManifestId ??
      host.probes[run.suite.replace("probe-", "") as ProbeId]
        ?.inputManifestId ??
      "sha256:" + "0".repeat(64)
    );
  }
  private async writeManifest(
    run: StoredRun,
    host: Host,
    payload: RunnerPayload,
    evidence: Manifest["evidence"],
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
      inputManifestId: this.inputManifestId(run, host),
      requestDigest: payload.requestDigest,
      toolIdentities: payload.toolIdentities,
      evidence,
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
    const target = resolve(dir, "manifest.json");
    const temporary = resolve(dir, `manifest-${randomUUID()}.tmp`);
    await writeFile(
      temporary,
      canonical({ manifest, signature: run.signature, algorithm: "Ed25519" }),
      { mode: 0o600, flag: "wx" },
    );
    await rename(temporary, target);
  }
}
