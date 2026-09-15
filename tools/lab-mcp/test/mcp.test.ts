import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import { test } from "node:test";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import type {
  Executor,
  HostChallenge,
  SignedJobRequest,
} from "../src/manager.js";
import { LabManager } from "../src/manager.js";
import { createServer } from "../src/server.js";
import { canonical, digest, signObject } from "../src/security.js";
import { config } from "./helpers.js";

const root = join(
  process.cwd(),
  ".trash",
  "test-runs",
  `lab-mcp-e2e-${process.pid}`,
);
await mkdir(root, { recursive: true });
test("real MCP client enumerates exact tools and resource templates", async () => {
  const setup = config(root);
  const executor: Executor = {
    async authenticate(_spec, challenge: HostChallenge) {
      const payload = {
        schemaVersion: 1 as const,
        hostId: challenge.hostId,
        challengeDigest: digest(canonical(challenge)),
        capabilities: ["wifi", "cpu-llvm"],
      };
      return canonical({
        payload,
        signature: signObject(payload, setup.hostKeys.privateKey),
        algorithm: "Ed25519",
      });
    },
    async execute(_spec, request: SignedJobRequest) {
      const payload = {
        schemaVersion: 1 as const,
        requestDigest: digest(canonical(request)),
        hostId: "lab-one",
        status: "succeeded" as const,
        startedAt: request.request.issuedAt,
        finishedAt: request.request.issuedAt,
        stdout: "ok",
        stderr: "",
        sanitization: "kyberia-lab-text-v2" as const,
        capabilities: ["wifi", "cpu-llvm"],
        toolIdentities: [
          {
            role: "git" as const,
            id: "git-test",
            version: "1",
            sha256: "0".repeat(64),
          },
          {
            role: "operation" as const,
            id: "operation-test",
            version: "1",
            sha256: "1".repeat(64),
          },
        ],
      };
      const signedPreimage = {
        schemaVersion: 1 as const,
        payloadDigest: digest(canonical(payload)),
      };
      return canonical({
        payload,
        signedPreimage,
        signature: signObject(signedPreimage, setup.hostKeys.privateKey),
        algorithm: "Ed25519",
      });
    },
  };
  const server = createServer(new LabManager(setup.config, executor));
  const client = new Client({ name: "kyberia-lab-test", version: "1.0.0" });
  const [clientTransport, serverTransport] =
    InMemoryTransport.createLinkedPair();
  await Promise.all([
    server.connect(serverTransport),
    client.connect(clientTransport),
  ]);
  const tools = (await client.listTools()).tools
    .map((tool) => tool.name)
    .sort();
  assert.deepEqual(
    tools,
    [
      "cancel_run",
      "fetch_artifact",
      "get_run_status",
      "probe_kismet",
      "probe_sionna",
      "probe_spectrum_source",
      "probe_wifi_capabilities",
      "run_kismet_contract_gate",
      "run_sionna_gate",
      "run_validation_suite",
    ].sort(),
  );
  const resources = await client.listResources();
  assert.deepEqual(
    resources.resources.map((r) => r.uri),
    ["lab://hosts"],
  );
  const templates = (await client.listResourceTemplates()).resourceTemplates
    .map((r) => r.uriTemplate)
    .sort();
  assert.deepEqual(
    templates,
    [
      "lab://hosts/{id}/capabilities",
      "lab://runs/{id}",
      "lab://runs/{id}/artifacts",
      "lab://runs/{id}/manifest",
    ].sort(),
  );
  const hosts = await client.readResource({ uri: "lab://hosts" });
  assert.match(
    hosts.contents[0] && "text" in hosts.contents[0]
      ? hosts.contents[0].text
      : "",
    /lab-one/,
  );
  const started = await client.callTool({
    name: "run_validation_suite",
    arguments: {
      host: "lab-one",
      git_sha: setup.config.immutableRevisions[0],
      suite: "foundation",
      seed: 7,
      timeout: 4,
    },
  });
  assert.equal(started.isError, undefined);
  assert.match(JSON.stringify(started), /run-/);
  const bad = await client.callTool({
    name: "run_validation_suite",
    arguments: {
      host: "lab-one",
      git_sha: "main",
      suite: "foundation",
      seed: 7,
      timeout: 4,
    },
  });
  assert.equal(bad.isError, true);
  const unknown = await client.callTool({
    name: "probe_wifi_capabilities",
    arguments: { host: "lab-one", command: "whoami" },
  });
  assert.equal(unknown.isError, true);
  const sionna = await client.callTool({
    name: "run_sionna_gate",
    arguments: { host: "lab-one", scene_set: "canonical-v1" },
  });
  assert.equal(sionna.isError, undefined);
  const retiredSionnaSelector = await client.callTool({
    name: "run_sionna_gate",
    arguments: {
      host: "lab-one",
      scene_set: "canonical-v1",
      cpu_or_gpu: "gpu",
    },
  });
  assert.equal(retiredSionnaSelector.isError, true);
  await client.close();
  await server.close();
});
