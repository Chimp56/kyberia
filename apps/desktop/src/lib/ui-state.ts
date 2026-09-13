import type { IpcErrorPayload, ProjectState } from "./contracts";

export type WorkspacePhase = "idle" | "loading" | "ready" | "error" | "unsupported";

export interface WorkspaceState {
  phase: WorkspacePhase;
  projectState: ProjectState;
  projectName: string;
  hasFloorPlan: boolean;
  calibrated: boolean;
  error: IpcErrorPayload | null;
  selectedTool: string;
  commandPaletteOpen: boolean;
  layerVisibility: Record<string, boolean>;
}

export const initialWorkspaceState: WorkspaceState = {
  phase: "idle",
  projectState: "no_project",
  projectName: "Untitled project",
  hasFloorPlan: false,
  calibrated: false,
  error: null,
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
