import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { loadConfig } from "./config.js";
import { LabManager } from "./manager.js";
import { createServer } from "./server.js";

const configPath = process.env.KYBERIA_LAB_CONFIG;
if (!configPath) throw new Error("KYBERIA_LAB_CONFIG is required");
const server = createServer(new LabManager(await loadConfig(configPath)));
await server.connect(new StdioServerTransport());
