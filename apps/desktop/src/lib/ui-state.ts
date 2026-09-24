import type { IpcErrorPayload, MapSummary, ProjectState } from "./contracts";

export type WorkspacePhase = "idle" | "loading" | "ready" | "error" | "unsupported";

export interface ActiveProjectJob {
  id: string;
  label: string;
  progress: number;
  state: "running" | "cancelling";
}

export interface ActiveProjectJobStatus {
  jobId: string;
  progress: number;
  state: ActiveProjectJob["state"];
}

export interface CommittedMapMutationReceipt {
  state: "committed";
  operationId: string;
  projectRevision: number;
  contentHash: string;
}

export interface MapReadbackRecovery {
  projectId: string | null;
  receipt: CommittedMapMutationReceipt;
  error: IpcErrorPayload;
}

export function mergeActiveProjectJobStatus(
  current: ActiveProjectJob | null,
  status: ActiveProjectJobStatus,
): ActiveProjectJob | null {
  if (current === null || current.id !== status.jobId) return current;
  return {
    ...current,
    progress: status.progress,
    state: current.state === "cancelling" ? "cancelling" : status.state,
  };
}

export interface WorkspaceState {
  phase: WorkspacePhase;
  projectState: ProjectState;
  projectId: string | null;
  projectName: string;
  hasFloorPlan: boolean;
  calibrated: boolean;
  floorId: string | null;
  maps: MapSummary[];
  error: IpcErrorPayload | null;
  readbackRecovery: MapReadbackRecovery | null;
  activeJob: ActiveProjectJob | null;
  selectedTool: string;
  commandPaletteOpen: boolean;
  layerVisibility: Record<string, boolean>;
}

export const initialWorkspaceState: WorkspaceState = {
  phase: "idle",
  projectState: "no_project",
  projectId: null,
  projectName: "Untitled project",
  hasFloorPlan: false,
  calibrated: false,
  floorId: null,
  maps: [],
  error: null,
  readbackRecovery: null,
  activeJob: null,
  selectedTool: "select",
  commandPaletteOpen: false,
  layerVisibility: {
    "floor-plan": true,
    observations: true,
    prediction: false,
    requirements: false,
  },
};

export function stateForError(error: IpcErrorPayload, previous: WorkspaceState = initialWorkspaceState): WorkspaceState {
  return {
    ...previous,
    phase: error.code === "capability_unavailable" ? "unsupported" : "error",
    error,
    activeJob: null,
  };
}

export function stateForMapReadbackFailure(
  receipt: CommittedMapMutationReceipt,
  error: IpcErrorPayload,
  previous: WorkspaceState,
): WorkspaceState {
  return {
    ...previous,
    phase: "error",
    error,
    readbackRecovery: { projectId: previous.projectId, receipt, error },
    activeJob: null,
  };
}

export function mapReadbackIsReconciled(
  projectId: string | null,
  projectRevision: number | null,
  recovery: MapReadbackRecovery,
): boolean {
  return projectId !== null
    && projectId === recovery.projectId
    && projectRevision !== null
    && projectRevision >= recovery.receipt.projectRevision;
}

export function createRequestGate() {
  let latest = 0;
  return {
    begin: () => {
      latest += 1;
      return latest;
    },
    isCurrent: (request: number) => request === latest,
  };
}
