import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { Config, type LabConfig } from "./schema.js";

export async function loadConfig(path: string): Promise<LabConfig> {
  const raw: unknown = JSON.parse(await readFile(resolve(path), "utf8"));
  const config = Config.parse(raw);
  return { ...config, stateDirectory: resolve(config.stateDirectory) };
}
