import type { IpcErrorPayload, ProjectState } from "./contracts";

export type WorkspacePhase = "idle" | "loading" | "ready" | "error" | "unsupported";

export interface WorkspaceState {
  phase: WorkspacePhase;
  projectState: ProjectState;
  projectName: string;
  error: IpcErrorPayload | null;
  selectedTool: string;
  commandPaletteOpen: boolean;
  layerVisibility: Record<string, boolean>;
}

export const initialWorkspaceState: WorkspaceState = {
  phase: "idle",
  projectState: "no_project",
  projectName: "Untitled project",
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

export function stateForError(error: IpcErrorPayload): WorkspaceState {
  return {
    ...initialWorkspaceState,
    phase: error.code === "capability_unavailable" ? "unsupported" : "error",
    error,
  };
}
