import { useCallback, useEffect, useRef, useState } from "react";
import { getDesktopIpc } from "./ipc";
import {
  assertJobCancelResponse,
  assertJobStatusResponse,
  assertMapMutationResponse,
  assertMapSourceSelectionResponse,
  assertOpenProjectSelectionResponse,
  assertResponse,
  normalizeIpcError,
  IPC_SCHEMA,
  type CurrentProjectResponse,
  type CalibrateMapRequest,
  type DesktopIpc,
  type IpcErrorPayload,
  type MapMutationResponse,
  type MapSourceSelection,
} from "./contracts";
import { createRequestGate, initialWorkspaceState, mergeActiveProjectJobStatus, stateForError, type WorkspaceState } from "./ui-state";

type ProjectOperation = "current" | "create" | "open" | "import" | "calibrate";
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
  importFloorPlan: () => Promise<void>;
  calibrateMap: (input: Omit<CalibrateMapRequest, "schema" | "jobId" | "operationId" | "actorId" | "deviceId" | "calibrationId"> & { calibrationId?: string }) => Promise<void>;
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
    floorId: response.project?.floorId ?? null,
    maps: response.project?.maps ?? [],
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
  const lastErrorRef = useRef<IpcErrorPayload | null>(null);
  const pendingMapSelection = useRef<MapSourceSelection | null>(null);
  const pendingCalibration = useRef<CalibrateMapRequest | null>(null);
  const actorId = useRef(crypto.randomUUID());
  const deviceId = useRef(crypto.randomUUID());

  const applyError = useCallback((error: IpcErrorPayload, operation: ProjectOperation | null) => {
    lastFailedOperation.current = operation;
    lastErrorRef.current = error;
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
            setState((current) => {
              const activeJob = mergeActiveProjectJobStatus(current.activeJob, status);
              return activeJob === current.activeJob ? current : { ...current, activeJob };
            });
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

  const runSelectionJob = useCallback(async (
    request: number,
    label: string,
  ): Promise<MapSourceSelection | null> => {
    if (activeJobId.current !== null) return null;
    const jobId = nextJobId();
    activeJobId.current = jobId;
    setState((current) => ({
      ...current,
      phase: "loading",
      error: null,
      activeJob: { id: jobId, label, progress: 0, state: "running" },
    }));
    const outcome = Promise.resolve()
      .then(() => ipc.selectMapSource({ schema: IPC_SCHEMA, jobId }))
      .then(
        (response) => ({ kind: "success" as const, response }),
        (error: unknown) => ({ kind: "failure" as const, error }),
      );
    try {
      while (true) {
        const observed = await Promise.race([outcome, delay(JOB_POLL_INTERVAL_MS)]);
        if (observed.kind === "success") {
          if (!requestGate.current.isCurrent(request)) return null;
          const selection = assertMapSourceSelectionResponse(observed.response).selection;
          if (selection === null) {
            lastErrorRef.current = null;
            setLastError(null);
            setState((current) => ({
              ...current,
              phase: current.projectState === "no_project" ? "idle" : "ready",
              error: null,
            }));
          }
          return selection;
        }
        if (observed.kind === "failure") throw observed.error;
        try {
          const status = assertJobStatusResponse(await ipc.jobStatus({ schema: IPC_SCHEMA, jobId }));
          if (status.jobId !== jobId) throw { schema: IPC_SCHEMA, code: "invalid_response", message: "The desktop adapter returned status for a different job.", retryable: false } satisfies IpcErrorPayload;
          if (requestGate.current.isCurrent(request)) {
            setState((current) => {
              const activeJob = mergeActiveProjectJobStatus(current.activeJob, status);
              return activeJob === current.activeJob ? current : { ...current, activeJob };
            });
          }
        } catch (value) {
          const statusError = normalizeIpcError(value);
          if (statusError.code !== "invalid_request") {
            try { await ipc.cancelJob({ schema: IPC_SCHEMA, jobId }); } catch { /* operation result is authoritative */ }
            throw statusError;
          }
        }
      }
    } catch (value) {
      if (requestGate.current.isCurrent(request)) applyError(normalizeIpcError(value), "import");
      return null;
    } finally {
      if (activeJobId.current === jobId) activeJobId.current = null;
      setState((current) => current.activeJob?.id === jobId ? { ...current, activeJob: null } : current);
    }
  }, [applyError, ipc]);

  const runMapMutationJob = useCallback(async (
    request: number,
    operation: "import" | "calibrate",
    label: string,
    invokeOperation: (jobId: string) => Promise<MapMutationResponse>,
  ): Promise<MapMutationResponse | null> => {
    if (activeJobId.current !== null) return null;
    const jobId = nextJobId();
    activeJobId.current = jobId;
    setState((current) => ({
      ...current,
      phase: "loading",
      error: null,
      activeJob: { id: jobId, label, progress: 0, state: "running" },
    }));
    const outcome = Promise.resolve()
      .then(() => invokeOperation(jobId))
      .then(
        (response) => ({ kind: "success" as const, response }),
        (error: unknown) => ({ kind: "failure" as const, error }),
      );
    try {
      while (true) {
        const observed = await Promise.race([outcome, delay(JOB_POLL_INTERVAL_MS)]);
        if (observed.kind === "success") {
          if (!requestGate.current.isCurrent(request)) return null;
          const response = assertMapMutationResponse(observed.response);
          if (response.current) {
            setState((current) => responseToState(response.current!, current));
          } else {
            setState((current) => ({ ...current, phase: "ready", error: response.readbackError, activeJob: null }));
          }
          lastErrorRef.current = null;
          setLastError(null);
          lastFailedOperation.current = null;
          return response;
        }
        if (observed.kind === "failure") throw observed.error;
        try {
          const status = assertJobStatusResponse(await ipc.jobStatus({ schema: IPC_SCHEMA, jobId }));
          if (status.jobId !== jobId) throw { schema: IPC_SCHEMA, code: "invalid_response", message: "The desktop adapter returned status for a different job.", retryable: false } satisfies IpcErrorPayload;
          if (requestGate.current.isCurrent(request)) {
            setState((current) => {
              const activeJob = mergeActiveProjectJobStatus(current.activeJob, status);
              return activeJob === current.activeJob ? current : { ...current, activeJob };
            });
          }
        } catch (value) {
          const statusError = normalizeIpcError(value);
          if (statusError.code !== "invalid_request") {
            try { await ipc.cancelJob({ schema: IPC_SCHEMA, jobId }); } catch { /* operation result is authoritative */ }
            throw statusError;
          }
        }
      }
    } catch (value) {
      if (requestGate.current.isCurrent(request)) applyError(normalizeIpcError(value), operation);
      return null;
    } finally {
      if (activeJobId.current === jobId) activeJobId.current = null;
      setState((current) => current.activeJob?.id === jobId ? { ...current, activeJob: null } : current);
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
            setState((current) => {
              const activeJob = mergeActiveProjectJobStatus(current.activeJob, status);
              return activeJob === current.activeJob ? current : { ...current, activeJob };
            });
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

  const importFloorPlan = useCallback(async () => {
    if (operationInFlight.current) return;
    operationInFlight.current = true;
    const request = requestGate.current.begin();
    try {
      let selection = pendingMapSelection.current;
      if (selection === null) {
        selection = await runSelectionJob(request, "Choosing PNG floor plan");
        if (selection === null) return;
        pendingMapSelection.current = selection;
      }
      const result = await runMapMutationJob(
        request,
        "import",
        "Importing PNG map",
        (jobId) => ipc.importMap({ schema: IPC_SCHEMA, jobId, grantId: selection!.grantId }),
      );
      if (result !== null) {
        pendingMapSelection.current = null;
      } else {
        const error = lastErrorRef.current;
        if (!error?.retryable || error.code === "cancelled" || error.code === "invalid_grant") {
          pendingMapSelection.current = null;
        }
      }
    } finally {
      operationInFlight.current = false;
    }
  }, [ipc, runMapMutationJob, runSelectionJob]);

  const performCalibration = useCallback(async (request: CalibrateMapRequest) => {
    if (operationInFlight.current) return;
    operationInFlight.current = true;
    const requestNumber = requestGate.current.begin();
    try {
      const result = await runMapMutationJob(
        requestNumber,
        "calibrate",
        "Calibrating map scale",
        (jobId) => ipc.calibrateMap({ ...request, schema: IPC_SCHEMA, jobId }),
      );
      if (result !== null) {
        pendingCalibration.current = null;
      } else if (!lastErrorRef.current?.retryable) {
        pendingCalibration.current = null;
      }
    } finally {
      operationInFlight.current = false;
    }
  }, [ipc, runMapMutationJob]);

  const calibrateMap = useCallback(async (
    input: Omit<CalibrateMapRequest, "schema" | "jobId" | "operationId" | "actorId" | "deviceId" | "calibrationId"> & { calibrationId?: string },
  ) => {
    if (operationInFlight.current) return;
    const pending = pendingCalibration.current;
    const sameIntent = pending !== null
      && pending.mapId === input.mapId
      && pending.firstXPixels === input.firstXPixels
      && pending.firstYPixels === input.firstYPixels
      && pending.secondXPixels === input.secondXPixels
      && pending.secondYPixels === input.secondYPixels
      && pending.knownDistanceMeters === input.knownDistanceMeters
      && (input.calibrationId === undefined || pending.calibrationId === input.calibrationId);
    const request = sameIntent && pending ? pending : {
      ...input,
      schema: IPC_SCHEMA,
      jobId: "",
      operationId: crypto.randomUUID(),
      actorId: actorId.current,
      deviceId: deviceId.current,
      calibrationId: input.calibrationId ?? crypto.randomUUID(),
    };
    pendingCalibration.current = request;
    await performCalibration(request);
  }, [performCalibration]);

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
    else if (lastFailedOperation.current === "import") await importFloorPlan();
    else if (lastFailedOperation.current === "calibrate" && pendingCalibration.current !== null) {
      await performCalibration(pendingCalibration.current);
    }
    else await runCurrent();
  }, [createBlankProject, importFloorPlan, lastError?.retryable, openProject, performCalibration, runCurrent]);

  return {
    state,
    createBlankProject,
    openProject,
    importFloorPlan,
    calibrateMap,
    retry,
    cancelActiveJob,
    selectTool,
    toggleLayer,
    setPaletteOpen,
  };
}
