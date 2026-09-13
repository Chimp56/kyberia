import { z } from "zod";

export const ID = z.string().regex(/^[a-z0-9][a-z0-9._-]{0,63}$/);
export const SHA = z.string().regex(/^[0-9a-f]{40}$/);
export const ARTIFACT = z.string().regex(/^[a-z0-9][a-z0-9._-]{0,127}$/);
export const SUITES = [
  "foundation",
  "capture",
  "kismet-contract",
  "sionna-cpu",
  "sionna-gpu",
  "spectrum",
] as const;
export const Suite = z.enum(SUITES);
export const Probe = z.enum(["wifi", "kismet", "sionna", "spectrum"]);

const command = z
  .object({
    executable: z.string().min(1).max(256),
    arguments: z.array(z.string().max(256)).max(32),
    version: z.string().min(1).max(64),
    environment: z
      .object({ KYBERIA_LAB_RUNNER_CONFIG: z.string().min(1).max(1024) })
      .strict(),
    credentialEnvNames: z
      .array(
        z.string().regex(/^KYBERIA_LAB_[A-Z0-9_]+_(?:PRIVATE|PUBLIC)_KEY$/),
      )
      .min(0)
      .max(4),
    inputManifestId: z.string().regex(/^sha256:[0-9a-f]{64}$/),
  })
  .strict()
  .superRefine((value, context) => {
    if (
      new Set(value.credentialEnvNames).size !== value.credentialEnvNames.length
    )
      context.addIssue({
        code: "custom",
        message: "credential environment names must be unique",
      });
  });

const host = z
  .object({
    id: ID,
    displayName: z.string().min(1).max(80),
    identity: z.string().regex(/^sha256:[A-Za-z0-9+/]{43}=$/),
    publicKeyEnv: z.string().regex(/^KYBERIA_LAB_[A-Z0-9_]+_PUBLIC_KEY$/),
    capabilities: z
      .array(ID)
      .max(64)
      .refine((v) => new Set(v).size === v.length),
    allowedKismetVersions: z
      .array(
        z
          .string()
          .regex(/^\d+\.\d+(?:\.\d+)?$/)
          .max(32),
      )
      .max(32),
    allowedFixtureSets: z.array(ID).max(64),
    allowedSceneSets: z.array(ID).max(64),
    suites: z.partialRecord(Suite, command),
    probes: z.partialRecord(Probe, command),
  })
  .strict();

export const Config = z
  .object({
    schemaVersion: z.literal(1),
    stateDirectory: z.string().min(1).max(1024),
    manifestPrivateKeyEnv: z
      .string()
      .regex(/^KYBERIA_LAB_[A-Z0-9_]+_PRIVATE_KEY$/),
    manifestPublicKeyEnv: z
      .string()
      .regex(/^KYBERIA_LAB_[A-Z0-9_]+_PUBLIC_KEY$/),
    coordinatorKeyId: ID,
    immutableRevisions: z
      .array(SHA)
      .min(1)
      .max(1024)
      .refine((v) => new Set(v).size === v.length),
    hosts: z.array(host).min(1).max(64),
    limits: z
      .object({
        concurrency: z.number().int().min(1).max(16),
        queue: z.number().int().min(1).max(128),
        timeoutSeconds: z.number().int().min(1).max(7200),
        outputBytes: z.number().int().min(1024).max(4_194_304),
        artifactBytes: z.number().int().min(1024).max(16_777_216),
        artifactCount: z.number().int().min(1).max(32),
        manifestBytes: z.number().int().min(1024).max(1_048_576),
      })
      .strict(),
  })
  .strict();

export type LabConfig = z.infer<typeof Config>;
export type SuiteId = z.infer<typeof Suite>;
export type ProbeId = z.infer<typeof Probe>;

const runnerCommand = z
  .object({
    executable: z.string().min(1).max(256),
    arguments: z.array(z.string().max(256)).max(32),
    version: z.string().min(1).max(64),
    parameters: z.record(
      z.string().max(64),
      z.array(z.string().max(64)).max(64),
    ),
    inputManifestId: z.string().regex(/^sha256:[0-9a-f]{64}$/),
  })
  .strict();
export const RunnerConfig = z
  .object({
    schemaVersion: z.literal(1),
    hostId: ID,
    hostIdentity: z.string().regex(/^sha256:[A-Za-z0-9+/]{43}=$/),
    hostPrivateKeyEnv: z.string().regex(/^KYBERIA_LAB_[A-Z0-9_]+_PRIVATE_KEY$/),
    coordinatorPublicKeyEnv: z
      .string()
      .regex(/^KYBERIA_LAB_[A-Z0-9_]+_PUBLIC_KEY$/),
    coordinatorKeyId: ID,
    checkoutDirectory: z.string().min(1).max(1024),
    replayDirectory: z.string().min(1).max(1024),
    maximumClockSkewSeconds: z.number().int().min(1).max(300),
    capabilities: z
      .array(ID)
      .max(64)
      .refine((v) => new Set(v).size === v.length),
    inputManifests: z.record(
      z.string().regex(/^sha256:[0-9a-f]{64}$/),
      z
        .array(
          z
            .object({
              path: z
                .string()
                .regex(/^(?!\/)(?!.*(?:^|\/)\.\.(?:\/|$))[A-Za-z0-9._\/-]+$/)
                .max(256),
              sha256: z.string().regex(/^[0-9a-f]{64}$/),
            })
            .strict(),
        )
        .min(1)
        .max(256)
        .refine((v) => new Set(v.map((e) => e.path)).size === v.length),
    ),
    limits: z
      .object({
        timeoutSeconds: z.number().int().min(1).max(7200),
        outputBytes: z.number().int().min(1024).max(4_194_304),
      })
      .strict(),
    operations: z.record(
      z.string().regex(/^[a-z0-9][a-z0-9._-]{0,63}$/),
      runnerCommand,
    ),
  })
  .strict();
export type LabRunnerConfig = z.infer<typeof RunnerConfig>;

export const validationInput = z
  .object({
    host: ID,
    git_sha: SHA,
    suite: Suite,
    seed: z.number().int().min(0).max(0xffff_ffff),
    timeout: z.number().int().min(1).max(7200),
  })
  .strict();

export const runInput = z.object({ run_id: ID }).strict();
export const hostInput = z.object({ host: ID }).strict();
export const kismetProbeInput = z
  .object({
    host: ID,
    expected_version: z
      .string()
      .regex(/^\d+\.\d+(?:\.\d+)?$/)
      .max(32),
  })
  .strict();
export const kismetGateInput = z.object({ host: ID, fixture_set: ID }).strict();
export const sionnaGateInput = z
  .object({ host: ID, cpu_or_gpu: z.enum(["cpu", "gpu"]), scene_set: ID })
  .strict();
export const artifactInput = z
  .object({ run_id: ID, artifact_name: ARTIFACT })
  .strict();
