import type { IpcErrorPayload, ProjectState } from "./contracts";

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
  error: IpcErrorPayload | null;
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
  error: null,
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
