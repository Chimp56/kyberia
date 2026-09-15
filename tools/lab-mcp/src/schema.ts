import { z } from "zod";

const runnerInvocation = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("direct") }).strict(),
  z
    .object({
      kind: z.literal("single-pinned-bundle"),
      bundlePath: z
        .string()
        .min(1)
        .max(1024)
        .regex(/^(?:\/|[A-Za-z]:\\)/),
      bundleSha256: z.string().regex(/^[0-9a-f]{64}$/),
    })
    .strict(),
]);

export const ID = z.string().regex(/^[a-z0-9][a-z0-9._-]{0,63}$/);
export const SHA = z.string().regex(/^[0-9a-f]{40}$/);
export const ARTIFACT = z.string().regex(/^[a-z0-9][a-z0-9._-]{0,127}$/);
export const SUITES = [
  "foundation",
  "capture",
  "kismet-contract",
  "sionna-cpu",
  "spectrum",
] as const;
export const Suite = z.enum(SUITES);
export const Probe = z.enum(["wifi", "kismet", "sionna", "spectrum"]);

const capabilities = z
  .array(ID)
  .max(64)
  .refine((v) => new Set(v).size === v.length)
  .refine(
    (v) => !v.some((id) => ["cuda", "gpu", "sionna-gpu"].includes(id)),
    "retired accelerator capabilities are unsupported",
  );

const command = z
  .object({
    executable: z
      .string()
      .min(1)
      .max(1024)
      .regex(/^(?:\/|[A-Za-z]:\\)/),
    executableSha256: z.string().regex(/^[0-9a-f]{64}$/),
    invocation: runnerInvocation,
    arguments: z.tuple([]),
    argumentFiles: z
      .array(
        z
          .object({
            argumentIndex: z.number().int().min(0).max(31),
            sha256: z.string().regex(/^[0-9a-f]{64}$/),
          })
          .strict(),
      )
      .max(0),
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
    const pinned = new Set(
      value.argumentFiles.map((item) => item.argumentIndex),
    );
    if (pinned.size !== value.argumentFiles.length)
      context.addIssue({
        code: "custom",
        message: "argument file indices must be unique",
      });
    for (const [index, argument] of value.arguments.entries())
      if (["-e", "--eval", "-c", "--command"].includes(argument))
        context.addIssue({
          code: "custom",
          message: "inline runner code arguments are forbidden",
        });
      else if (
        (/^(?:\/|[A-Za-z]:\\)/.test(argument) ||
          /\.(?:[cm]?js|tsx?|py|sh|bash|zsh|ps1)$/i.test(argument)) &&
        !pinned.has(index)
      )
        context.addIssue({
          code: "custom",
          message: "absolute runner arguments must be digest pinned",
        });
    for (const index of pinned)
      if (
        !value.arguments[index] ||
        !/^(?:\/|[A-Za-z]:\\)/.test(value.arguments[index]!)
      )
        context.addIssue({
          code: "custom",
          message: "pinned argument must be an absolute path",
        });
  });

const host = z
  .object({
    id: ID,
    displayName: z.string().min(1).max(80),
    identity: z.string().regex(/^sha256:[A-Za-z0-9+/]{43}=$/),
    publicKeyEnv: z.string().regex(/^KYBERIA_LAB_[A-Z0-9_]+_PUBLIC_KEY$/),
    capabilities,
    allowedKismetVersions: z
      .array(
        z
          .string()
          .regex(/^\d+\.\d+(?:\.\d+)?$/)
          .max(32),
      )
      .max(32)
      .refine((v) => new Set(v).size === v.length),
    allowedFixtureSets: z
      .array(ID)
      .max(64)
      .refine((v) => new Set(v).size === v.length),
    allowedSceneSets: z
      .array(ID)
      .max(64)
      .refine((v) => new Set(v).size === v.length),
    suites: z.partialRecord(Suite, command),
    probes: z.partialRecord(Probe, command),
  })
  .strict();

export const Config = z
  .object({
    schemaVersion: z.literal(2),
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
    hosts: z
      .array(host)
      .min(1)
      .max(64)
      .refine((v) => new Set(v.map((item) => item.id)).size === v.length)
      .refine((v) => new Set(v.map((item) => item.identity)).size === v.length),
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
  .strict()
  .superRefine((value, context) => {
    if (value.manifestPrivateKeyEnv === value.manifestPublicKeyEnv)
      context.addIssue({
        code: "custom",
        message: "coordinator key roles must use distinct names",
      });
    for (const hostValue of value.hosts)
      for (const spec of [
        ...Object.values(hostValue.suites),
        ...Object.values(hostValue.probes),
      ])
        if (spec?.credentialEnvNames.includes(value.manifestPrivateKeyEnv))
          context.addIssue({
            code: "custom",
            message: "coordinator private signing key cannot be forwarded",
          });
    for (const hostValue of value.hosts)
      if (
        hostValue.publicKeyEnv === value.manifestPublicKeyEnv ||
        hostValue.publicKeyEnv === value.manifestPrivateKeyEnv
      )
        context.addIssue({
          code: "custom",
          message: "host and coordinator key roles must be distinct",
        });
  });

export type LabConfig = z.infer<typeof Config>;
export type SuiteId = z.infer<typeof Suite>;
export type ProbeId = z.infer<typeof Probe>;

const runnerCommand = z
  .object({
    toolId: ID,
    executable: z
      .string()
      .min(1)
      .max(1024)
      .regex(/^(?:\/|[A-Za-z]:\\)/),
    executableSha256: z.string().regex(/^[0-9a-f]{64}$/),
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
    gitExecutable: z
      .string()
      .min(1)
      .max(1024)
      .regex(/^(?:\/|[A-Za-z]:\\)/),
    gitExecutableSha256: z.string().regex(/^[0-9a-f]{64}$/),
    gitToolId: ID,
    gitVersion: z.string().min(1).max(64),
    checkoutDirectory: z.string().min(1).max(1024),
    replayDirectory: z.string().min(1).max(1024),
    maximumClockSkewSeconds: z.number().int().min(1).max(300),
    capabilities,
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
              mode: z.enum(["100644", "100755"]),
            })
            .strict(),
        )
        .min(1)
        .max(20_000)
        .refine((v) => new Set(v.map((e) => e.path)).size === v.length),
    ),
    limits: z
      .object({
        timeoutSeconds: z.number().int().min(1).max(7200),
        outputBytes: z.number().int().min(1024).max(4_194_304),
        inputBytes: z.number().int().min(1).max(1_073_741_824),
      })
      .strict(),
    operations: z.record(
      z.string().regex(/^[a-z0-9][a-z0-9._-]{0,63}$/),
      runnerCommand,
    ),
  })
  .strict()
  .superRefine((value, context) => {
    if (value.hostPrivateKeyEnv === value.coordinatorPublicKeyEnv)
      context.addIssue({
        code: "custom",
        message: "runner key roles must use distinct names",
      });
    const toolIds = [
      value.gitToolId,
      ...Object.values(value.operations).map((operation) => operation.toolId),
    ];
    if (new Set(toolIds).size !== toolIds.length)
      context.addIssue({
        code: "custom",
        message: "runner tool IDs must be unique",
      });
  });
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
export const sionnaGateInput = z.object({ host: ID, scene_set: ID }).strict();
export const artifactInput = z
  .object({ run_id: ID, artifact_name: ARTIFACT })
  .strict();
