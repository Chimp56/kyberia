import { useCallback, useEffect, useRef, useState } from "react";
import { getDesktopIpc } from "./ipc";
import { assertOpenProjectSelectionResponse, assertResponse, normalizeIpcError, IPC_SCHEMA, type CurrentProjectResponse, type DesktopIpc, type IpcErrorPayload } from "./contracts";
import { createRequestGate, initialWorkspaceState, stateForError, type WorkspaceState } from "./ui-state";

export interface ProjectSessionController {
  state: WorkspaceState;
  createBlankProject: () => Promise<void>;
  openProject: () => Promise<void>;
  importFloorPlan: () => void;
  retry: () => Promise<void>;
  selectTool: (tool: string) => void;
  toggleLayer: (layer: string) => void;
  setPaletteOpen: (open: boolean) => void;
}

export function responseToState(response: CurrentProjectResponse, previous: WorkspaceState): WorkspaceState {
  return {
    ...previous,
    phase: "ready",
    projectState: response.state,
    projectName: response.project?.name ?? previous.projectName,
    hasFloorPlan: response.project?.hasFloorPlan ?? false,
    calibrated: response.project?.calibrated ?? false,
    error: null,
  };
}

export function useProjectSession(ipc: DesktopIpc = getDesktopIpc()): ProjectSessionController {
  const [state, setState] = useState<WorkspaceState>(initialWorkspaceState);
  const [lastError, setLastError] = useState<IpcErrorPayload | null>(null);
  const requestGate = useRef(createRequestGate());

  const applyError = useCallback((error: IpcErrorPayload) => {
    setLastError(error);
    setState((current) => stateForError(error, current));
  }, []);

  const runCurrent = useCallback(async () => {
    const request = requestGate.current.begin();
    setState((current) => ({ ...current, phase: "loading", error: null }));
    try {
      const response = assertResponse(await ipc.currentProject());
      if (!requestGate.current.isCurrent(request)) return;
      setState((current) => responseToState(response, current));
      setLastError(null);
    } catch (value) {
      const error = normalizeIpcError(value);
      if (requestGate.current.isCurrent(request)) applyError(error);
    }
  }, [applyError, ipc]);

  useEffect(() => {
    // Browser preview deliberately stays local and empty. A Tauri window can
    // query the canonical application boundary as soon as it is mounted.
    if (typeof window !== "undefined" && window.__TAURI_INTERNALS__) void runCurrent();
  }, [runCurrent]);

  const createBlankProject = useCallback(async () => {
    const request = requestGate.current.begin();
    setState((current) => ({ ...current, phase: "loading", error: null }));
    try {
      const response = assertResponse(await ipc.createBlankProject({ schema: IPC_SCHEMA, name: "Untitled project" }));
      if (!requestGate.current.isCurrent(request)) return;
      setState((current) => responseToState(response, current));
      setLastError(null);
    } catch (value) {
      const error = normalizeIpcError(value);
      if (requestGate.current.isCurrent(request)) applyError(error);
    }
  }, [applyError, ipc]);

  const openProject = useCallback(async () => {
    const request = requestGate.current.begin();
    setState((current) => ({ ...current, phase: "loading", error: null }));
    try {
      const selection = assertOpenProjectSelectionResponse(await ipc.selectOpenProject());
      if (!requestGate.current.isCurrent(request) || selection.selection === null) {
        if (requestGate.current.isCurrent(request)) setState((current) => ({ ...current, phase: "idle", error: null }));
        return;
      }
      const response = assertResponse(await ipc.openProject({
        schema: IPC_SCHEMA,
        grantId: selection.selection.grantId,
        mode: "read_write",
        expectedName: selection.selection.displayName,
      }));
      if (!requestGate.current.isCurrent(request)) return;
      setState((current) => responseToState(response, { ...current, projectName: selection.selection?.displayName ?? current.projectName }));
      setLastError(null);
    } catch (value) {
      const error = normalizeIpcError(value);
      if (requestGate.current.isCurrent(request)) applyError(error);
    }
  }, [applyError, ipc]);

  const importFloorPlan = useCallback(() => {
    const error: IpcErrorPayload = {
      schema: IPC_SCHEMA,
      code: "capability_unavailable",
      message: "Floor-plan import is not yet exposed by the application command boundary.",
      remediation: "Create a blank project now, or use a build with the ImportFloorPlan command enabled.",
      retryable: false,
    };
    applyError(error);
  }, [applyError]);

  const selectTool = useCallback((selectedTool: string) => {
    setState((current) => ({ ...current, selectedTool }));
  }, []);
  const toggleLayer = useCallback((layer: string) => {
    setState((current) => ({ ...current, layerVisibility: { ...current.layerVisibility, [layer]: !current.layerVisibility[layer] } }));
  }, []);
  const setPaletteOpen = useCallback((commandPaletteOpen: boolean) => {
    setState((current) => ({ ...current, commandPaletteOpen }));
  }, []);

  return {
    state,
    createBlankProject,
    openProject,
    importFloorPlan,
    retry: lastError?.retryable ? runCurrent : createBlankProject,
    selectTool,
    toggleLayer,
    setPaletteOpen,
  };
}
