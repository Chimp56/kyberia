import { invoke } from "@tauri-apps/api/core";
import {
  assertResponse,
  type CreateBlankProjectRequest,
  type CreateProjectRequest,
  type CurrentProjectResponse,
  IPC_SCHEMA,
  type DesktopIpc,
  type OpenProjectRequest,
} from "./contracts";

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
    __RF_ATLAS_IPC__?: DesktopIpc;
  }
}

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && window.__TAURI_INTERNALS__ !== undefined;
}

async function invokeProject(command: string, payload?: unknown): Promise<CurrentProjectResponse> {
  if (!isTauriRuntime()) {
    throw {
      schema: IPC_SCHEMA,
      code: "capability_unavailable",
      message: "Project commands are available from the RF Atlas desktop shell.",
      remediation: "Open this workspace in the Tauri desktop build to create or open a project.",
      retryable: false,
    };
  }
  return assertResponse(await invoke<unknown>(command, payload as Record<string, unknown> | undefined));
}

const tauriIpc: DesktopIpc = {
  createBlankProject: (request: CreateBlankProjectRequest) => invokeProject("project_create_blank", request),
  createProject: (request: CreateProjectRequest) => invokeProject("project_create", request),
  openProject: (request: OpenProjectRequest) => invokeProject("project_open", request),
  currentProject: () => invokeProject("project_current", { schema: IPC_SCHEMA }),
};

export function getDesktopIpc(): DesktopIpc {
  return window.__RF_ATLAS_IPC__ ?? tauriIpc;
}
