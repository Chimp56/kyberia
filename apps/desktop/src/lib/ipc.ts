import { invoke } from "@tauri-apps/api/core";
import {
  assertResponse,
  assertJobCancelResponse,
  assertJobStatusResponse,
  assertMapMutationResponse,
  assertMapSourceSelectionResponse,
  assertOpenProjectSelectionResponse,
  type CreateBlankProjectRequest,
  type CurrentProjectResponse,
  type CalibrateMapRequest,
  IPC_SCHEMA,
  type DesktopIpc,
  type JobRequest,
  type MapGrantRequest,
  type MapMutationResponse,
  type MapSourceSelectionResponse,
  type OpenProjectGrantRequest,
  type OpenProjectSelectionResponse,
  type SelectOpenProjectRequest,
  type SelectMapSourceRequest,
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

async function invokeProject<T>(command: string, payload: unknown, validate: (value: unknown) => T): Promise<T> {
  if (!isTauriRuntime()) {
    throw {
      schema: IPC_SCHEMA,
      code: "capability_unavailable",
      message: "Project commands are available from the RF Atlas desktop shell.",
      remediation: "Open this workspace in the Tauri desktop build to create or open a project.",
      retryable: false,
    };
  }
  return validate(await invoke<unknown>(command, payload as Record<string, unknown> | undefined));
}

const tauriIpc: DesktopIpc = {
  createBlankProject: (request: CreateBlankProjectRequest) => invokeProject("project_create_blank", request, assertResponse),
  selectOpenProject: (request: SelectOpenProjectRequest) => invokeProject<OpenProjectSelectionResponse>("project_select_open", request, assertOpenProjectSelectionResponse),
  openProject: (request: OpenProjectGrantRequest) => invokeProject("project_open_grant", request, assertResponse),
  selectMapSource: (request: SelectMapSourceRequest) => invokeProject<MapSourceSelectionResponse>("project_select_map", request, assertMapSourceSelectionResponse),
  importMap: (request: MapGrantRequest) => invokeProject<MapMutationResponse>("project_import_map", request, assertMapMutationResponse),
  calibrateMap: (request: CalibrateMapRequest) => invokeProject<MapMutationResponse>("project_calibrate_map", request, assertMapMutationResponse),
  currentProject: (request: JobRequest) => invokeProject("project_current", request, assertResponse),
  jobStatus: (request: JobRequest) => invokeProject("project_job_status", request, assertJobStatusResponse),
  cancelJob: (request: JobRequest) => invokeProject("project_cancel", request, assertJobCancelResponse),
};

export function getDesktopIpc(): DesktopIpc {
  return window.__RF_ATLAS_IPC__ ?? tauriIpc;
}
