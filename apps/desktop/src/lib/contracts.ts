export const IPC_SCHEMA = "kyberia.desktop-ipc/1" as const;

export type ProjectState = "no_project" | "materialized_current" | "baseline_only" | "legacy_absent";
export type CapabilityState = "available" | "conditional" | "unavailable" | "unknown";

export interface ProjectSummary {
  projectId: string;
  name: string;
  state: Exclude<ProjectState, "no_project">;
  schemaVersion: "1" | "2";
  revision: number;
  logicalTime: number;
  hasFloorPlan: boolean;
}

export interface CurrentProjectResponse {
  schema: typeof IPC_SCHEMA;
  state: ProjectState;
  project: ProjectSummary | null;
  capabilities: CapabilitySummary[];
}

export interface CapabilitySummary {
  id: string;
  label: string;
  state: CapabilityState;
  detail: string;
  remediation?: string;
}

export interface IpcErrorPayload {
  schema: typeof IPC_SCHEMA;
  code: string;
  message: string;
  remediation?: string;
  retryable: boolean;
}

export interface CreateBlankProjectRequest {
  schema: typeof IPC_SCHEMA;
  name: string;
}

export interface CreateProjectRequest {
  schema: typeof IPC_SCHEMA;
  path: string;
  name: string;
  createdUtcMs: number;
}

export interface OpenProjectRequest {
  schema: typeof IPC_SCHEMA;
  path: string;
  mode: "read_only" | "read_write";
}

export interface DesktopIpc {
  createBlankProject(request: CreateBlankProjectRequest): Promise<CurrentProjectResponse>;
  createProject(request: CreateProjectRequest): Promise<CurrentProjectResponse>;
  openProject(request: OpenProjectRequest): Promise<CurrentProjectResponse>;
  currentProject(): Promise<CurrentProjectResponse>;
}

export function isIpcErrorPayload(value: unknown): value is IpcErrorPayload {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<IpcErrorPayload>;
  return candidate.schema === IPC_SCHEMA && typeof candidate.code === "string" && typeof candidate.message === "string";
}

export function normalizeIpcError(value: unknown): IpcErrorPayload {
  if (isIpcErrorPayload(value)) return value;
  if (value instanceof Error) {
    return {
      schema: IPC_SCHEMA,
      code: "desktop_command_failed",
      message: value.message,
      retryable: true,
    };
  }
  return {
    schema: IPC_SCHEMA,
    code: "desktop_command_failed",
    message: typeof value === "string" ? value : "The desktop command failed without a structured error.",
    retryable: true,
  };
}

export function assertResponse(value: unknown): CurrentProjectResponse {
  if (!value || typeof value !== "object" || (value as { schema?: unknown }).schema !== IPC_SCHEMA) {
    throw {
      schema: IPC_SCHEMA,
      code: "unsupported_version",
      message: "The desktop adapter returned an unknown response schema.",
      remediation: "Update RF Atlas before opening this project.",
      retryable: false,
    } satisfies IpcErrorPayload;
  }
  return value as CurrentProjectResponse;
}
