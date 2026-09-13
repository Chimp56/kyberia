import { useCallback, useEffect, useRef, useState } from "react";
import { getDesktopIpc } from "./ipc";
import {
  assertJobCancelResponse,
  assertJobStatusResponse,
  assertOpenProjectSelectionResponse,
  assertResponse,
  normalizeIpcError,
  IPC_SCHEMA,
  type CurrentProjectResponse,
  type DesktopIpc,
  type IpcErrorPayload,
} from "./contracts";
import { createRequestGate, initialWorkspaceState, stateForError, type WorkspaceState } from "./ui-state";

type ProjectOperation = "current" | "create" | "open";
type JobOutcome =
  | { kind: "success"; response: CurrentProjectResponse }
  | { kind: "failure"; error: unknown };

const JOB_POLL_INTERVAL_MS = 50;

function nextJobId(): string {
  return crypto.randomUUID();
}

function delay(milliseconds: number): Promise<{ kind: "poll" }> {
  return new Promise((resolve) => window.setTimeout(() => resolve({ kind: "poll" }), milliseconds));
}

export interface ProjectSessionController {
  state: WorkspaceState;
  createBlankProject: () => Promise<void>;
  openProject: () => Promise<void>;
  importFloorPlan: () => void;
  retry: () => Promise<void>;
  cancelActiveJob: () => Promise<void>;
  selectTool: (tool: string) => void;
  toggleLayer: (layer: string) => void;
  setPaletteOpen: (open: boolean) => void;
}

export function responseToState(response: CurrentProjectResponse, previous: WorkspaceState): WorkspaceState {
  return {
    ...previous,
    phase: "ready",
    projectState: response.state,
    projectId: response.project?.projectId ?? null,
    projectName: response.project?.name ?? previous.projectName,
    hasFloorPlan: response.project?.hasFloorPlan ?? false,
    calibrated: response.project?.calibrated ?? false,
    error: null,
    activeJob: null,
  };
}

export function useProjectSession(ipc: DesktopIpc = getDesktopIpc()): ProjectSessionController {
  const [state, setState] = useState<WorkspaceState>(initialWorkspaceState);
  const [lastError, setLastError] = useState<IpcErrorPayload | null>(null);
  const requestGate = useRef(createRequestGate());
  const activeJobId = useRef<string | null>(null);
  const operationInFlight = useRef(false);
  const lastFailedOperation = useRef<ProjectOperation | null>(null);

  const applyError = useCallback((error: IpcErrorPayload, operation: ProjectOperation | null) => {
    lastFailedOperation.current = operation;
    setLastError(error);
    setState((current) => stateForError(error, current));
  }, []);

  const runJob = useCallback(async (
    request: number,
    operation: ProjectOperation,
    label: string,
    invokeOperation: (jobId: string) => Promise<CurrentProjectResponse>,
    reserved = false,
  ): Promise<void> => {
    if (!reserved) {
      if (operationInFlight.current) return;
      operationInFlight.current = true;
    }
    if (activeJobId.current !== null) {
      if (!reserved) operationInFlight.current = false;
      return;
    }
    const jobId = nextJobId();
    activeJobId.current = jobId;
    setState((current) => ({
      ...current,
      phase: "loading",
      error: null,
      activeJob: { id: jobId, label, progress: 0, state: "running" },
    }));

    const outcome: Promise<JobOutcome> = Promise.resolve().then(() => invokeOperation(jobId)).then(
      (response) => ({ kind: "success", response }),
      (error: unknown) => ({ kind: "failure", error }),
    );

    try {
      while (true) {
        const observed = await Promise.race([outcome, delay(JOB_POLL_INTERVAL_MS)]);
        if (observed.kind === "success") {
          if (!requestGate.current.isCurrent(request)) return;
          setState((current) => responseToState(assertResponse(observed.response), current));
          setLastError(null);
          lastFailedOperation.current = null;
          return;
        }
        if (observed.kind === "failure") throw observed.error;

        try {
          const status = assertJobStatusResponse(await ipc.jobStatus({ schema: IPC_SCHEMA, jobId }));
          if (status.jobId !== jobId) throw { schema: IPC_SCHEMA, code: "invalid_response", message: "The desktop adapter returned status for a different job.", retryable: false } satisfies IpcErrorPayload;
          if (requestGate.current.isCurrent(request)) {
            setState((current) => current.activeJob?.id === jobId
              ? { ...current, activeJob: { ...current.activeJob, progress: status.progress, state: status.state } }
              : current);
          }
        } catch (value) {
          const statusError = normalizeIpcError(value);
          // Completion can remove the job between the operation race and this
          // status request. The command result remains authoritative.
          if (statusError.code !== "invalid_request") {
            try {
              assertJobCancelResponse(await ipc.cancelJob({ schema: IPC_SCHEMA, jobId }));
            } catch {
              // Preserve the original malformed-status error for the UI.
            }
            throw statusError;
          }
        }
      }
    } catch (value) {
      if (requestGate.current.isCurrent(request)) applyError(normalizeIpcError(value), operation);
    } finally {
      if (activeJobId.current === jobId) activeJobId.current = null;
      setState((current) => current.activeJob?.id === jobId ? { ...current, activeJob: null } : current);
      if (!reserved) operationInFlight.current = false;
    }
  }, [applyError, ipc]);

  const runCurrent = useCallback(async () => {
    if (operationInFlight.current) return;
    const request = requestGate.current.begin();
    await runJob(request, "current", "Opening project", (jobId) => ipc.currentProject({ schema: IPC_SCHEMA, jobId }));
  }, [ipc, runJob]);

  useEffect(() => {
    // Browser preview deliberately stays local and empty. A Tauri window can
    // query the canonical application boundary as soon as it is mounted.
    if (typeof window !== "undefined" && window.__TAURI_INTERNALS__) void runCurrent();
  }, [runCurrent]);

  const createBlankProject = useCallback(async () => {
    if (operationInFlight.current) return;
    const request = requestGate.current.begin();
    await runJob(request, "create", "Creating project", (jobId) => ipc.createBlankProject({
      schema: IPC_SCHEMA,
      jobId,
      name: "Untitled project",
    }));
  }, [ipc, runJob]);

  const openProject = useCallback(async () => {
    if (operationInFlight.current) return;
    const request = requestGate.current.begin();
    const pickerJobId = nextJobId();
    operationInFlight.current = true;
    activeJobId.current = pickerJobId;
    setState((current) => ({
      ...current,
      phase: "loading",
      error: null,
      activeJob: { id: pickerJobId, label: "Choosing project", progress: 5, state: "running" },
    }));
    let selectionOutcome: Promise<{ kind: "success"; response: unknown } | { kind: "failure"; error: unknown }>;
    try {
      selectionOutcome = Promise.resolve()
        .then(() => ipc.selectOpenProject({ schema: IPC_SCHEMA, jobId: pickerJobId }))
        .then((response) => ({ kind: "success", response }), (error: unknown) => ({ kind: "failure", error }));
      while (true) {
        const observed = await Promise.race([selectionOutcome, delay(JOB_POLL_INTERVAL_MS)]);
        if (observed.kind === "success") {
          const selection = assertOpenProjectSelectionResponse(observed.response);
          if (!requestGate.current.isCurrent(request)) return;
          if (selection.selection === null) {
            setState((current) => ({
              ...current,
              phase: current.projectState === "no_project" ? "idle" : "ready",
              error: null,
              activeJob: null,
            }));
            return;
          }
          const selected = selection.selection;
          activeJobId.current = null;
          setState((current) => ({ ...current, activeJob: null }));
          await runJob(request, "open", "Opening project", (jobId) => ipc.openProject({
            schema: IPC_SCHEMA,
            jobId,
            grantId: selected.grantId,
            mode: "read_write",
            expectedName: selected.displayName,
          }), true);
          return;
        }
        if (observed.kind === "failure") throw observed.error;
        try {
          const status = assertJobStatusResponse(await ipc.jobStatus({ schema: IPC_SCHEMA, jobId: pickerJobId }));
          if (status.jobId !== pickerJobId) throw { schema: IPC_SCHEMA, code: "invalid_response", message: "The desktop adapter returned status for a different job.", retryable: false } satisfies IpcErrorPayload;
          if (requestGate.current.isCurrent(request)) {
            setState((current) => current.activeJob?.id === pickerJobId
              ? { ...current, activeJob: { ...current.activeJob, progress: status.progress, state: status.state } }
              : current);
          }
        } catch (value) {
          const statusError = normalizeIpcError(value);
          if (statusError.code !== "invalid_request") throw statusError;
        }
      }
    } catch (value) {
      if (requestGate.current.isCurrent(request)) applyError(normalizeIpcError(value), "open");
    } finally {
      if (activeJobId.current === pickerJobId) {
        activeJobId.current = null;
        setState((current) => current.activeJob?.id === pickerJobId ? { ...current, activeJob: null } : current);
      }
      operationInFlight.current = false;
    }
  }, [applyError, ipc, runJob]);

  const importFloorPlan = useCallback(() => {
    const error: IpcErrorPayload = {
      schema: IPC_SCHEMA,
      code: "capability_unavailable",
      message: "Floor-plan import is not yet exposed by the application command boundary.",
      remediation: "Create a project now, or use a build with the ImportFloorPlan command enabled.",
      retryable: false,
    };
    applyError(error, null);
  }, [applyError]);

  const cancelActiveJob = useCallback(async () => {
    const jobId = activeJobId.current;
    if (jobId === null) return;
    setState((current) => current.activeJob?.id === jobId
      ? { ...current, activeJob: { ...current.activeJob, state: "cancelling" } }
      : current);
    try {
      const response = assertJobCancelResponse(await ipc.cancelJob({ schema: IPC_SCHEMA, jobId }));
      if (response.jobId !== jobId) throw { schema: IPC_SCHEMA, code: "invalid_response", message: "The desktop adapter cancelled a different job.", retryable: false } satisfies IpcErrorPayload;
    } catch (value) {
      const error = normalizeIpcError(value);
      // A just-completed command wins the race with a late Cancel click.
      if (error.code !== "invalid_request") applyError(error, lastFailedOperation.current);
    }
  }, [applyError, ipc]);

  const selectTool = useCallback((selectedTool: string) => {
    setState((current) => ({ ...current, selectedTool }));
  }, []);
  const toggleLayer = useCallback((layer: string) => {
    setState((current) => ({ ...current, layerVisibility: { ...current.layerVisibility, [layer]: !current.layerVisibility[layer] } }));
  }, []);
  const setPaletteOpen = useCallback((commandPaletteOpen: boolean) => {
    setState((current) => ({ ...current, commandPaletteOpen }));
  }, []);

  const retry = useCallback(async () => {
    if (!lastError?.retryable) return;
    if (lastFailedOperation.current === "create") await createBlankProject();
    else if (lastFailedOperation.current === "open") await openProject();
    else await runCurrent();
  }, [createBlankProject, lastError?.retryable, openProject, runCurrent]);

  return {
    state,
    createBlankProject,
    openProject,
    importFloorPlan,
    retry,
    cancelActiveJob,
    selectTool,
    toggleLayer,
    setPaletteOpen,
  };
}
