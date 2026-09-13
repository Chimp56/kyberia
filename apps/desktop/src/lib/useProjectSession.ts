import { useCallback, useEffect, useState } from "react";
import { getDesktopIpc } from "./ipc";
import { normalizeIpcError, IPC_SCHEMA, type CurrentProjectResponse, type DesktopIpc, type IpcErrorPayload } from "./contracts";
import { initialWorkspaceState, stateForError, type WorkspaceState } from "./ui-state";

export interface ProjectSessionController {
  state: WorkspaceState;
  createBlankProject: () => Promise<void>;
  importFloorPlan: () => void;
  retry: () => Promise<void>;
  selectTool: (tool: string) => void;
  toggleLayer: (layer: string) => void;
  setPaletteOpen: (open: boolean) => void;
}

function responseToState(response: CurrentProjectResponse, previous: WorkspaceState): WorkspaceState {
  return {
    ...previous,
    phase: "ready",
    projectState: response.state,
    projectName: response.project?.name ?? previous.projectName,
    error: null,
  };
}

export function useProjectSession(ipc: DesktopIpc = getDesktopIpc()): ProjectSessionController {
  const [state, setState] = useState<WorkspaceState>(initialWorkspaceState);
  const [lastError, setLastError] = useState<IpcErrorPayload | null>(null);

  const runCurrent = useCallback(async () => {
    setState((current) => ({ ...current, phase: "loading", error: null }));
    try {
      const response = await ipc.currentProject();
      setState((current) => responseToState(response, current));
      setLastError(null);
    } catch (value) {
      const error = normalizeIpcError(value);
      setLastError(error);
      setState((current) => ({ ...stateForError(error), projectName: current.projectName, selectedTool: current.selectedTool, layerVisibility: current.layerVisibility }));
    }
  }, [ipc]);

  useEffect(() => {
    // Browser preview deliberately stays local and empty. A Tauri window can
    // query the canonical application boundary as soon as it is mounted.
    if (typeof window !== "undefined" && window.__TAURI_INTERNALS__) void runCurrent();
  }, [runCurrent]);

  const createBlankProject = useCallback(async () => {
    setState((current) => ({ ...current, phase: "loading", error: null }));
    try {
      const response = await ipc.createBlankProject({ schema: IPC_SCHEMA, name: "Untitled project" });
      setState((current) => responseToState(response, current));
      setLastError(null);
    } catch (value) {
      const error = normalizeIpcError(value);
      setLastError(error);
      setState((current) => ({ ...stateForError(error), projectName: current.projectName, selectedTool: current.selectedTool, layerVisibility: current.layerVisibility }));
    }
  }, [ipc]);

  const importFloorPlan = useCallback(() => {
    const error: IpcErrorPayload = {
      schema: IPC_SCHEMA,
      code: "capability_unavailable",
      message: "Floor-plan import is not yet exposed by the application command boundary.",
      remediation: "Create a blank project now, or use a build with the ImportFloorPlan command enabled.",
      retryable: false,
    };
    setLastError(error);
    setState((current) => ({ ...stateForError(error), projectName: current.projectName, selectedTool: current.selectedTool, layerVisibility: current.layerVisibility }));
  }, []);

  return {
    state,
    createBlankProject,
    importFloorPlan,
    retry: lastError?.retryable ? runCurrent : createBlankProject,
    selectTool: (selectedTool) => setState((current) => ({ ...current, selectedTool })),
    toggleLayer: (layer) => setState((current) => ({ ...current, layerVisibility: { ...current.layerVisibility, [layer]: !current.layerVisibility[layer] } })),
    setPaletteOpen: (commandPaletteOpen) => setState((current) => ({ ...current, commandPaletteOpen })),
  };
}
