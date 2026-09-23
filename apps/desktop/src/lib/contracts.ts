export const IPC_SCHEMA = "kyberia.desktop-ipc/2" as const;

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
  floorId: string | null;
  maps: MapSummary[];
}

export interface MapSummary {
  mapId: string;
  floorId: string;
  name: string;
  width: number;
  height: number;
  calibrated: boolean;
  metersPerPixel: number | null;
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
  jobId: string;
  name: string;
}

export interface SelectOpenProjectRequest {
  schema: typeof IPC_SCHEMA;
  jobId: string;
}

export interface OpenProjectGrantRequest {
  schema: typeof IPC_SCHEMA;
  jobId: string;
  grantId: string;
  mode: "read_only" | "read_write";
  expectedName?: string;
}

export interface JobRequest {
  schema: typeof IPC_SCHEMA;
  jobId: string;
}

export interface SelectMapSourceRequest {
  schema: typeof IPC_SCHEMA;
  jobId: string;
}

export interface MapGrantRequest extends JobRequest {
  grantId: string;
}

export interface MapSourceSelection {
  grantId: string;
  displayName: string;
  byteLength: number;
  kind: "png";
}

export interface MapSourceSelectionResponse {
  schema: typeof IPC_SCHEMA;
  selection: MapSourceSelection | null;
}

export interface CalibrateMapRequest extends JobRequest {
  operationId: string;
  actorId: string;
  deviceId: string;
  mapId: string;
  calibrationId: string;
  firstXPixels: number;
  firstYPixels: number;
  secondXPixels: number;
  secondYPixels: number;
  knownDistanceMeters: number;
}

export interface MapMutationResponse {
  schema: typeof IPC_SCHEMA;
  state: "committed";
  operationId: string;
  projectRevision: number;
  contentHash: string;
  current: CurrentProjectResponse | null;
  readbackError: IpcErrorPayload | null;
}

export interface JobStatusResponse {
  schema: typeof IPC_SCHEMA;
  jobId: string;
  state: "running" | "cancelling";
  progress: number;
}

export interface JobCancelResponse {
  schema: typeof IPC_SCHEMA;
  jobId: string;
  state: "cancelling";
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
  selectOpenProject(request: SelectOpenProjectRequest): Promise<OpenProjectSelectionResponse>;
  openProject(request: OpenProjectGrantRequest): Promise<CurrentProjectResponse>;
  selectMapSource(request: SelectMapSourceRequest): Promise<MapSourceSelectionResponse>;
  importMap(request: MapGrantRequest): Promise<MapMutationResponse>;
  calibrateMap(request: CalibrateMapRequest): Promise<MapMutationResponse>;
  currentProject(request: JobRequest): Promise<CurrentProjectResponse>;
  jobStatus(request: JobRequest): Promise<JobStatusResponse>;
  cancelJob(request: JobRequest): Promise<JobCancelResponse>;
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
  return hasOnlyKeys(value, ["projectId", "name", "state", "schemaVersion", "revision", "logicalTime", "hasFloorPlan", "calibrated", "floorId", "maps"])
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
    && typeof candidate.calibrated === "boolean"
    && (candidate.floorId === null || (typeof candidate.floorId === "string" && candidate.floorId.length > 0))
    && Array.isArray(candidate.maps)
    && candidate.maps.every(isMapSummary)
    && candidate.hasFloorPlan === (candidate.maps.length > 0)
    && candidate.calibrated === candidate.maps.some((map) => map.calibrated);
}

function isMapSummary(value: unknown): value is MapSummary {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<MapSummary>;
  return hasOnlyKeys(value, ["mapId", "floorId", "name", "width", "height", "calibrated", "metersPerPixel"])
    && typeof candidate.mapId === "string" && candidate.mapId.length > 0
    && typeof candidate.floorId === "string" && candidate.floorId.length > 0
    && typeof candidate.name === "string" && candidate.name.length > 0
    && typeof candidate.width === "number" && Number.isSafeInteger(candidate.width) && candidate.width > 0
    && typeof candidate.height === "number" && Number.isSafeInteger(candidate.height) && candidate.height > 0
    && typeof candidate.calibrated === "boolean"
    && (candidate.metersPerPixel === null || (typeof candidate.metersPerPixel === "number" && Number.isFinite(candidate.metersPerPixel) && candidate.metersPerPixel > 0))
    && candidate.calibrated === (candidate.metersPerPixel !== null);
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

function isMapSourceSelection(value: unknown): value is MapSourceSelection {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<MapSourceSelection>;
  return hasOnlyKeys(value, ["grantId", "displayName", "byteLength", "kind"])
    && typeof candidate.grantId === "string" && candidate.grantId.length > 0
    && typeof candidate.displayName === "string" && candidate.displayName.length > 0
    && typeof candidate.byteLength === "number" && Number.isSafeInteger(candidate.byteLength)
    && candidate.byteLength > 0 && candidate.byteLength <= 32 * 1024 * 1024
    && candidate.kind === "png";
}

export function assertMapSourceSelectionResponse(value: unknown): MapSourceSelectionResponse {
  if (!value || typeof value !== "object") throw invalidResponse("The desktop adapter returned an invalid PNG selection.");
  const candidate = value as Partial<MapSourceSelectionResponse>;
  if (!hasOnlyKeys(value, ["schema", "selection"])
    || candidate.schema !== IPC_SCHEMA
    || !(candidate.selection === null || isMapSourceSelection(candidate.selection))) {
    throw invalidResponse("The desktop adapter returned an invalid PNG selection.");
  }
  return candidate as MapSourceSelectionResponse;
}

export function assertMapMutationResponse(value: unknown): MapMutationResponse {
  if (!value || typeof value !== "object") throw invalidResponse("The desktop adapter returned an invalid map mutation receipt.");
  const candidate = value as Partial<MapMutationResponse>;
  const hasReadback = candidate.current === null
    ? candidate.readbackError !== null
    : candidate.readbackError === null && isCurrentProjectResponse(candidate.current);
  if (!hasOnlyKeys(value, ["schema", "state", "operationId", "projectRevision", "contentHash", "current", "readbackError"])
    || candidate.schema !== IPC_SCHEMA
    || candidate.state !== "committed"
    || typeof candidate.operationId !== "string" || candidate.operationId.length === 0
    || typeof candidate.projectRevision !== "number" || !Number.isSafeInteger(candidate.projectRevision) || candidate.projectRevision < 1
    || typeof candidate.contentHash !== "string" || !/^[0-9a-f]{64}$/.test(candidate.contentHash)
    || !(candidate.readbackError === null || isIpcErrorPayload(candidate.readbackError))
    || !(candidate.current === null || isCurrentProjectResponse(candidate.current))
    || !hasReadback) {
    throw invalidResponse("The desktop adapter returned an invalid map mutation receipt.");
  }
  return candidate as MapMutationResponse;
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

function isCanonicalJobId(value: unknown): value is string {
  return typeof value === "string"
    && /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(value);
}

export function assertJobStatusResponse(value: unknown): JobStatusResponse {
  if (!value || typeof value !== "object" || (value as { schema?: unknown }).schema !== IPC_SCHEMA) {
    throw invalidResponse("The desktop adapter returned an unknown job-status schema.");
  }
  const candidate = value as Partial<JobStatusResponse>;
  if (!hasOnlyKeys(value, ["schema", "jobId", "state", "progress"])
    || !isCanonicalJobId(candidate.jobId)
    || (candidate.state !== "running" && candidate.state !== "cancelling")
    || typeof candidate.progress !== "number"
    || !Number.isSafeInteger(candidate.progress)
    || candidate.progress < 0
    || candidate.progress > 100) {
    throw invalidResponse("The desktop adapter returned a malformed job status.");
  }
  return value as JobStatusResponse;
}

export function assertJobCancelResponse(value: unknown): JobCancelResponse {
  if (!value || typeof value !== "object" || (value as { schema?: unknown }).schema !== IPC_SCHEMA) {
    throw invalidResponse("The desktop adapter returned an unknown cancellation schema.");
  }
  const candidate = value as Partial<JobCancelResponse>;
  if (!hasOnlyKeys(value, ["schema", "jobId", "state"])
    || !isCanonicalJobId(candidate.jobId)
    || candidate.state !== "cancelling") {
    throw invalidResponse("The desktop adapter returned a malformed cancellation response.");
  }
  return value as JobCancelResponse;
}

function scrubRendererMessage(message: string): string {
  if (message.split(/\s+/).some((part) => /^\\\\/.test(part) || /^\//.test(part) || /^[A-Za-z]:[\\/]/.test(part))) {
    return "The desktop command failed while accessing a local project.";
  }
  return message;
}
