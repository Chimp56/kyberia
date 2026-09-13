import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { test } from "node:test";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { inputManifestId } from "../src/runner.js";
import { canonical } from "../src/security.js";
import { keys } from "./helpers.js";

test(
  "real stdio MCP uses ProcessExecutor, authenticated runner, seed, and manifest",
  {
    skip:
      process.platform === "win32"
        ? "native Job Object runner is fail-closed"
        : false,
  },
  async () => {
    const state = join(
      process.cwd(),
      ".trash",
      "test-runs",
      `lab-stdio-${process.pid}`,
    );
    await mkdir(state, { recursive: true });
    const coordinator = keys("STDIO_COORDINATOR");
    const host = keys("STDIO_HOST");
    const repository = resolve(process.cwd(), "../..");
    const gitExecutable = "/usr/bin/git";
    const fileSha256 = async (path: string) =>
      createHash("sha256")
        .update(await readFile(path))
        .digest("hex");
    const sha = execFileSync(gitExecutable, ["rev-parse", "HEAD"], {
      cwd: repository,
      encoding: "utf8",
    }).trim();
    const input = execFileSync(
      gitExecutable,
      ["ls-tree", "-rz", "--full-tree", sha],
      { cwd: repository },
    )
      .toString("utf8")
      .split("\0")
      .filter(Boolean)
      .map((record) => {
        const match = /^(100644|100755) blob ([0-9a-f]{40,64})\t(.+)$/.exec(
          record,
        );
        assert.ok(match);
        return {
          path: match[3]!,
          mode: match[1]! as "100644" | "100755",
          sha256: createHash("sha256")
            .update(
              execFileSync(gitExecutable, ["cat-file", "blob", match[2]!], {
                cwd: repository,
                maxBuffer: 64_000_000,
              }),
            )
            .digest("hex"),
        };
      });
    const manifestId = inputManifestId(input);
    const tsx = resolve("node_modules/tsx/dist/loader.mjs");
    const runnerPath = join(state, "runner.json");
    const coordinatorPath = join(state, "coordinator.json");
    await writeFile(
      runnerPath,
      canonical({
        schemaVersion: 1,
        hostId: "stdio-host",
        hostIdentity: host.identity,
        hostPrivateKeyEnv: host.privateName,
        coordinatorPublicKeyEnv: coordinator.publicName,
        coordinatorKeyId: "stdio-coordinator",
        gitExecutable,
        gitExecutableSha256: await fileSha256(gitExecutable),
        gitToolId: "git-system",
        gitVersion: "git-test",
        checkoutDirectory: repository,
        replayDirectory: join(state, "replay"),
        maximumClockSkewSeconds: 60,
        capabilities: ["stdio"],
        inputManifests: { [manifestId]: input },
        limits: {
          timeoutSeconds: 30,
          outputBytes: 4096,
          inputBytes: 64_000_000,
        },
        operations: {
          foundation: {
            toolId: "printenv",
            executable: "/usr/bin/printenv",
            executableSha256: await fileSha256("/usr/bin/printenv"),
            arguments: ["KYBERIA_LAB_SEED"],
            version: "foundation-v1",
            parameters: { default: [] },
            inputManifestId: manifestId,
          },
        },
      }),
    );
    const runnerSpec = {
      executable: process.execPath,
      executableSha256: await fileSha256(process.execPath),
      arguments: ["--import", tsx, resolve("src/runner.ts")],
      argumentFiles: [
        { argumentIndex: 1, sha256: await fileSha256(tsx) },
        {
          argumentIndex: 2,
          sha256: await fileSha256(resolve("src/runner.ts")),
        },
      ],
      version: "foundation-v1",
      environment: { KYBERIA_LAB_RUNNER_CONFIG: runnerPath },
      credentialEnvNames: [host.privateName, coordinator.publicName],
      inputManifestId: manifestId,
    };
    await writeFile(
      coordinatorPath,
      canonical({
        schemaVersion: 1,
        stateDirectory: join(state, "runs"),
        manifestPrivateKeyEnv: coordinator.privateName,
        manifestPublicKeyEnv: coordinator.publicName,
        coordinatorKeyId: "stdio-coordinator",
        immutableRevisions: [sha],
        hosts: [
          {
            id: "stdio-host",
            displayName: "Stdio host",
            identity: host.identity,
            publicKeyEnv: host.publicName,
            capabilities: ["stdio"],
            allowedKismetVersions: [],
            allowedFixtureSets: [],
            allowedSceneSets: [],
            suites: { foundation: runnerSpec },
            probes: {},
          },
        ],
        limits: {
          concurrency: 1,
          queue: 2,
          timeoutSeconds: 30,
          outputBytes: 4096,
          artifactBytes: 4096,
          artifactCount: 2,
          manifestBytes: 16384,
        },
      }),
    );
    const client = new Client({ name: "stdio-contract", version: "1.0.0" });
    const transport = new StdioClientTransport({
      command: process.execPath,
      args: ["--import", tsx, resolve("src/main.ts")],
      env: {
        ...process.env,
        KYBERIA_LAB_CONFIG: coordinatorPath,
      } as Record<string, string>,
    });
    await client.connect(transport);
    try {
      const started = await client.callTool({
        name: "run_validation_suite",
        arguments: {
          host: "stdio-host",
          git_sha: sha,
          suite: "foundation",
          seed: 11,
          timeout: 25,
        },
      });
      const first = (
        started as { content: Array<{ type: string; text: string }> }
      ).content[0];
      assert.ok(first && first.type === "text");
      const runId = (JSON.parse(first.text) as { run_id: string }).run_id;
      let status = "";
      for (let i = 0; i < 3_000; i++) {
        const current = await client.callTool({
          name: "get_run_status",
          arguments: { run_id: runId },
        });
        const content = (
          current as { content: Array<{ type: string; text: string }> }
        ).content[0];
        assert.ok(content && content.type === "text");
        status = (JSON.parse(content.text) as { status: string }).status;
        if (!["queued", "running", "cancelling"].includes(status)) break;
        await new Promise((done) => setTimeout(done, 10));
      }
      assert.equal(status, "succeeded");
      const artifact = await client.callTool({
        name: "fetch_artifact",
        arguments: { run_id: runId, artifact_name: "stdout.txt" },
      });
      assert.match(JSON.stringify(artifact), /11/);
      const manifest = await client.readResource({
        uri: `lab://runs/${runId}/manifest`,
      });
      const text =
        manifest.contents[0] && "text" in manifest.contents[0]
          ? manifest.contents[0].text
          : "";
      assert.match(text, /Ed25519/);
      assert.match(text, /foundation-v1/);
      assert.match(text, new RegExp(manifestId));
    } finally {
      await client.close();
    }
  },
);
