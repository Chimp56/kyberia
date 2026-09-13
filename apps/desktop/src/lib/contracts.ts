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
  calibrated: boolean;
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

export interface OpenProjectGrantRequest {
  schema: typeof IPC_SCHEMA;
  grantId: string;
  mode: "read_only" | "read_write";
  expectedName?: string;
}

export interface OpenProjectSelection {
  grantId: string;
  displayName: string;
  kind: "open";
}

export interface OpenProjectSelectionResponse {
  schema: typeof IPC_SCHEMA;
  selection: OpenProjectSelection | null;
}

export interface DesktopIpc {
  createBlankProject(request: CreateBlankProjectRequest): Promise<CurrentProjectResponse>;
  selectOpenProject(): Promise<OpenProjectSelectionResponse>;
  openProject(request: OpenProjectGrantRequest): Promise<CurrentProjectResponse>;
  currentProject(): Promise<CurrentProjectResponse>;
}

export function isIpcErrorPayload(value: unknown): value is IpcErrorPayload {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<IpcErrorPayload>;
  return hasOnlyKeys(value, ["schema", "code", "message", "remediation", "retryable"])
    && candidate.schema === IPC_SCHEMA
    && typeof candidate.code === "string"
    && candidate.code.length > 0
    && typeof candidate.message === "string"
    && candidate.message.length > 0
    && typeof candidate.retryable === "boolean"
    && (candidate.remediation === undefined || typeof candidate.remediation === "string");
}

function hasOnlyKeys(value: object, keys: string[]): boolean {
  return Object.keys(value).every((key) => keys.includes(key));
}

function invalidResponse(message: string): IpcErrorPayload {
  return {
    schema: IPC_SCHEMA,
    code: "invalid_response",
    message,
    remediation: "Update RF Atlas before opening this project.",
    retryable: false,
  };
}

function isProjectState(value: unknown): value is ProjectState {
  return value === "no_project" || value === "materialized_current" || value === "baseline_only" || value === "legacy_absent";
}

function isCapabilityState(value: unknown): value is CapabilityState {
  return value === "available" || value === "conditional" || value === "unavailable" || value === "unknown";
}

function isProjectSummary(value: unknown): value is ProjectSummary {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<ProjectSummary>;
  return hasOnlyKeys(value, ["projectId", "name", "state", "schemaVersion", "revision", "logicalTime", "hasFloorPlan", "calibrated"])
    && typeof candidate.projectId === "string"
    && candidate.projectId.length > 0
    && typeof candidate.name === "string"
    && candidate.name.length > 0
    && typeof candidate.state === "string"
    && isProjectState(candidate.state)
    && (candidate.schemaVersion === "1" || candidate.schemaVersion === "2")
    && typeof candidate.revision === "number"
    && Number.isSafeInteger(candidate.revision)
    && candidate.revision >= 0
    && typeof candidate.logicalTime === "number"
    && Number.isSafeInteger(candidate.logicalTime)
    && candidate.logicalTime >= 0
    && typeof candidate.hasFloorPlan === "boolean"
    && typeof candidate.calibrated === "boolean";
}

function isCapabilitySummary(value: unknown): value is CapabilitySummary {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<CapabilitySummary>;
  return hasOnlyKeys(value, ["id", "label", "state", "detail", "remediation"])
    && typeof candidate.id === "string"
    && candidate.id.length > 0
    && typeof candidate.label === "string"
    && candidate.label.length > 0
    && isCapabilityState(candidate.state)
    && typeof candidate.detail === "string"
    && candidate.detail.length > 0
    && (candidate.remediation === undefined || typeof candidate.remediation === "string");
}

function isCurrentProjectResponse(value: unknown): value is CurrentProjectResponse {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<CurrentProjectResponse>;
  return hasOnlyKeys(value, ["schema", "state", "project", "capabilities"])
    && candidate.schema === IPC_SCHEMA
    && isProjectState(candidate.state)
    && (candidate.project === null || isProjectSummary(candidate.project))
    && Array.isArray(candidate.capabilities)
    && candidate.capabilities.every(isCapabilitySummary)
    && (candidate.project === null || candidate.project.state === candidate.state)
    && ((candidate.state === "no_project" && candidate.project === null)
      || (candidate.state !== "no_project" && candidate.project !== null));
}

function isOpenProjectSelection(value: unknown): value is OpenProjectSelection {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<OpenProjectSelection>;
  return hasOnlyKeys(value, ["grantId", "displayName", "kind"])
    && typeof candidate.grantId === "string"
    && candidate.grantId.length > 0
    && typeof candidate.displayName === "string"
    && candidate.displayName.length > 0
    && candidate.kind === "open";
}

export function normalizeIpcError(value: unknown): IpcErrorPayload {
  if (isIpcErrorPayload(value)) {
    return {
      ...value,
      message: scrubRendererMessage(value.message),
      remediation: value.remediation === undefined ? undefined : scrubRendererMessage(value.remediation),
    };
  }
  if (value && typeof value === "object" && (value as { schema?: unknown }).schema === IPC_SCHEMA) {
    return invalidResponse("The desktop adapter returned a malformed error payload.");
  }
  if (value instanceof Error) {
    return {
      schema: IPC_SCHEMA,
      code: "desktop_command_failed",
      message: scrubRendererMessage(value.message),
      retryable: true,
    };
  }
  return {
    schema: IPC_SCHEMA,
    code: "desktop_command_failed",
    message: typeof value === "string" ? scrubRendererMessage(value) : "The desktop command failed without a structured error.",
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
  if (!isCurrentProjectResponse(value)) throw invalidResponse("The desktop adapter returned a malformed project response.");
  return value;
}

export function assertOpenProjectSelectionResponse(value: unknown): OpenProjectSelectionResponse {
  if (!value || typeof value !== "object" || (value as { schema?: unknown }).schema !== IPC_SCHEMA) {
    throw {
      schema: IPC_SCHEMA,
      code: "unsupported_version",
      message: "The desktop adapter returned an unknown selection schema.",
      remediation: "Update RF Atlas before opening this project.",
      retryable: false,
    } satisfies IpcErrorPayload;
  }
  const candidate = value as Partial<OpenProjectSelectionResponse>;
  if (!hasOnlyKeys(value, ["schema", "selection"]) || !(candidate.selection === null || isOpenProjectSelection(candidate.selection))) {
    throw invalidResponse("The desktop adapter returned a malformed project selection.");
  }
  return value as OpenProjectSelectionResponse;
}

function scrubRendererMessage(message: string): string {
  if (message.split(/\s+/).some((part) => /^\//.test(part) || /^[A-Za-z]:[\\/]/.test(part))) {
    return "The desktop command failed while accessing a local project.";
  }
  return message;
}
