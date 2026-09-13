import {
  McpServer,
  ResourceTemplate,
} from "@modelcontextprotocol/sdk/server/mcp.js";
import { z } from "zod";
import type { LabManager } from "./manager.js";
import { ARTIFACT, ID, SHA, Suite } from "./schema.js";

const json = (value: unknown) => ({
  content: [{ type: "text" as const, text: JSON.stringify(value) }],
  structuredContent: value as Record<string, unknown>,
});
const resource = (uri: URL, value: unknown) => ({
  contents: [
    {
      uri: uri.href,
      mimeType: "application/json",
      text: JSON.stringify(value),
    },
  ],
});
export function createServer(manager: LabManager): McpServer {
  const server = new McpServer({ name: "kyberia-lab", version: "0.1.0" });
  server.registerResource(
    "hosts",
    "lab://hosts",
    { title: "Authenticated lab hosts", mimeType: "application/json" },
    async (uri) => resource(uri, manager.listHosts()),
  );
  server.registerResource(
    "host-capabilities",
    new ResourceTemplate("lab://hosts/{id}/capabilities", { list: undefined }),
    { title: "Pinned host capabilities", mimeType: "application/json" },
    async (uri, values) =>
      resource(uri, manager.capabilities(ID.parse(values.id))),
  );
  server.registerResource(
    "run",
    new ResourceTemplate("lab://runs/{id}", { list: undefined }),
    { title: "Lab run status", mimeType: "application/json" },
    async (uri, values) => resource(uri, manager.status(ID.parse(values.id))),
  );
  server.registerResource(
    "run-manifest",
    new ResourceTemplate("lab://runs/{id}/manifest", { list: undefined }),
    { title: "Signed run manifest", mimeType: "application/json" },
    async (uri, values) => resource(uri, manager.manifest(ID.parse(values.id))),
  );
  server.registerResource(
    "run-artifacts",
    new ResourceTemplate("lab://runs/{id}/artifacts", { list: undefined }),
    { title: "Sanitized artifact inventory", mimeType: "application/json" },
    async (uri, values) =>
      resource(uri, manager.artifacts(ID.parse(values.id))),
  );

  server.registerTool(
    "run_validation_suite",
    {
      description:
        "Run one administrator-allowlisted validation suite at an admitted immutable revision.",
      inputSchema: z
        .object({
          host: ID,
          git_sha: SHA,
          suite: Suite,
          seed: z.number().int().min(0).max(0xffff_ffff),
          timeout: z.number().int().min(1).max(7200),
        })
        .strict(),
    },
    async (input) =>
      json({
        run_id: await manager.submit(
          input.host,
          input.git_sha,
          input.suite,
          input.seed,
          input.timeout,
        ),
      }),
  );
  server.registerTool(
    "get_run_status",
    {
      inputSchema: z.object({ run_id: ID }).strict(),
      annotations: { readOnlyHint: true },
    },
    async ({ run_id }) => json(manager.status(run_id)),
  );
  server.registerTool(
    "cancel_run",
    {
      inputSchema: z.object({ run_id: ID }).strict(),
      annotations: { destructiveHint: true },
    },
    async ({ run_id }) => json(manager.cancel(run_id)),
  );
  server.registerTool(
    "probe_wifi_capabilities",
    { inputSchema: z.object({ host: ID }).strict() },
    async ({ host }) => json({ run_id: await manager.probe(host, "wifi") }),
  );
  server.registerTool(
    "probe_kismet",
    {
      inputSchema: z
        .object({
          host: ID,
          expected_version: z
            .string()
            .regex(/^\d+\.\d+(?:\.\d+)?$/)
            .max(32),
        })
        .strict(),
    },
    async ({ host, expected_version }) =>
      json({
        run_id: await manager.probe(host, "kismet", { expected_version }),
      }),
  );
  server.registerTool(
    "run_kismet_contract_gate",
    { inputSchema: z.object({ host: ID, fixture_set: ID }).strict() },
    async ({ host, fixture_set }) =>
      json({ run_id: await manager.probe(host, "kismet", { fixture_set }) }),
  );
  server.registerTool(
    "probe_sionna",
    { inputSchema: z.object({ host: ID }).strict() },
    async ({ host }) => json({ run_id: await manager.probe(host, "sionna") }),
  );
  server.registerTool(
    "run_sionna_gate",
    {
      inputSchema: z
        .object({ host: ID, cpu_or_gpu: z.enum(["cpu", "gpu"]), scene_set: ID })
        .strict(),
    },
    async ({ host, cpu_or_gpu, scene_set }) =>
      json({
        run_id: await manager.probe(host, "sionna", { cpu_or_gpu, scene_set }),
      }),
  );
  server.registerTool(
    "probe_spectrum_source",
    { inputSchema: z.object({ host: ID }).strict() },
    async ({ host }) => json({ run_id: await manager.probe(host, "spectrum") }),
  );
  server.registerTool(
    "fetch_artifact",
    {
      inputSchema: z.object({ run_id: ID, artifact_name: ARTIFACT }).strict(),
      annotations: { readOnlyHint: true },
    },
    async ({ run_id, artifact_name }) =>
      json(await manager.fetch(run_id, artifact_name)),
  );
  return server;
}
