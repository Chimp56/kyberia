import { createHash, generateKeyPairSync } from "node:crypto";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import type { LabConfig } from "../src/schema.js";
import { canonical, digest } from "../src/security.js";

export function keys(prefix: string) {
  const pair = generateKeyPairSync("ed25519");
  const privateDer = pair.privateKey.export({ format: "der", type: "pkcs8" });
  const publicDer = pair.publicKey.export({ format: "der", type: "spki" });
  const privateName = `KYBERIA_LAB_${prefix}_PRIVATE_KEY`,
    publicName = `KYBERIA_LAB_${prefix}_PUBLIC_KEY`;
  process.env[privateName] = privateDer.toString("base64");
  process.env[publicName] = publicDer.toString("base64");
  const identity = `sha256:${createHash("sha256").update(publicDer).digest("base64")}`;
  return { privateName, publicName, identity, privateKey: pair.privateKey };
}

export function config(state: string): {
  config: LabConfig;
  hostKeys: ReturnType<typeof keys>;
} {
  const coordinator = keys("TEST_COORDINATOR"),
    hostKeys = keys("TEST_HOST");
  const input = [
    {
      path: "README.md",
      sha256: createHash("sha256")
        .update(readFileSync(resolve(process.cwd(), "../..", "README.md")))
        .digest("hex"),
      mode: "100644" as const,
    },
  ];
  const inputManifestId = digest(canonical(input));
  const spec = (version: string) => ({
    executable: "/fixed/runner",
    executableSha256: "0".repeat(64),
    arguments: ["--stdio"],
    version,
    environment: { KYBERIA_LAB_RUNNER_CONFIG: "/fixed/runner.json" },
    credentialEnvNames: [hostKeys.privateName],
    inputManifestId,
  });
  return {
    hostKeys,
    config: {
      schemaVersion: 1,
      stateDirectory: resolve(state),
      manifestPrivateKeyEnv: coordinator.privateName,
      manifestPublicKeyEnv: coordinator.publicName,
      coordinatorKeyId: "test-coordinator",
      immutableRevisions: ["0123456789abcdef0123456789abcdef01234567"],
      hosts: [
        {
          id: "lab-one",
          displayName: "Test lab",
          identity: hostKeys.identity,
          publicKeyEnv: hostKeys.publicName,
          capabilities: ["wifi", "cuda"],
          allowedKismetVersions: ["2025.01"],
          allowedFixtureSets: ["golden-v1"],
          allowedSceneSets: ["canonical-v1"],
          suites: {
            foundation: spec("foundation-v1"),
          },
          probes: {
            wifi: spec("wifi-v1"),
            kismet: spec("kismet-v1"),
            sionna: spec("sionna-v1"),
            spectrum: spec("spectrum-v1"),
          },
        },
      ],
      limits: {
        concurrency: 1,
        queue: 2,
        timeoutSeconds: 30,
        outputBytes: 1024,
        artifactBytes: 1024,
        artifactCount: 2,
        manifestBytes: 16384,
      },
    },
  };
}
