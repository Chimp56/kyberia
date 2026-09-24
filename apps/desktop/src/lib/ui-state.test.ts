import { describe, expect, it } from "vitest";
import { createRequestGate, initialWorkspaceState, mapReadbackIsReconciled, mergeActiveProjectJobStatus, stateForError, stateForMapReadbackFailure } from "./ui-state";

describe("workspace state semantics", () => {
  it("starts without inventing a project or measurements", () => {
    expect(initialWorkspaceState.projectState).toBe("no_project");
    expect(initialWorkspaceState.projectId).toBeNull();
    expect(initialWorkspaceState.phase).toBe("idle");
    expect(initialWorkspaceState.activeJob).toBeNull();
    expect(initialWorkspaceState.layerVisibility.prediction).toBe(false);
  });

  it("makes unsupported capability explicit", () => {
    const state = stateForError({
      schema: "kyberia.desktop-ipc/2",
      code: "capability_unavailable",
      message: "Live capture is unavailable.",
      retryable: false,
    });
    expect(state.phase).toBe("unsupported");
    expect(state.error?.message).toContain("unavailable");
  });

  it("only lets the latest async request commit", () => {
    const gate = createRequestGate();
    const first = gate.begin();
    const second = gate.begin();
    expect(gate.isCurrent(second)).toBe(true);
    expect(gate.isCurrent(first)).toBe(false);
  });

  it("ignores a first response that resolves after the second response", async () => {
    const gate = createRequestGate();
    const committed: string[] = [];
    let resolveFirst!: (value: string) => void;
    let resolveSecond!: (value: string) => void;
    const first = new Promise<string>((resolve) => { resolveFirst = resolve; });
    const second = new Promise<string>((resolve) => { resolveSecond = resolve; });
    const firstRequest = gate.begin();
    const secondRequest = gate.begin();
    const commit = (request: number, value: string) => { if (gate.isCurrent(request)) committed.push(value); };
    resolveSecond("new");
    commit(secondRequest, await second);
    resolveFirst("old");
    commit(firstRequest, await first);
    expect(committed).toEqual(["new"]);
  });

  it("preserves the open project when an import capability is unsupported", () => {
    const current = { ...initialWorkspaceState, projectState: "baseline_only" as const, projectName: "Office", hasFloorPlan: false, calibrated: false };
    const next = stateForError({
      schema: "kyberia.desktop-ipc/2",
      code: "capability_unavailable",
      message: "Import is unavailable.",
      retryable: false,
    }, current);
    expect(next.projectState).toBe("baseline_only");
    expect(next.projectName).toBe("Office");
  });

  it("keeps a committed receipt and marks the cached view stale after readback failure", () => {
    const previous = {
      ...initialWorkspaceState,
      phase: "ready" as const,
      projectState: "materialized_current" as const,
      projectId: "project-1",
      projectName: "Office",
      hasFloorPlan: true,
      maps: [{ mapId: "map-1", floorId: "floor-1", name: "Old plan.png", width: 200, height: 120, calibrated: false, metersPerPixel: null }],
    };
    const receipt = {
      state: "committed" as const,
      operationId: "operation-7",
      projectRevision: 7,
      contentHash: "a".repeat(64),
    };
    const error = {
      schema: "kyberia.desktop-ipc/2" as const,
      code: "storage",
      message: "The canonical view could not be queried.",
      retryable: false,
    };

    const recovery = stateForMapReadbackFailure(receipt, error, previous);
    expect(recovery.phase).toBe("error");
    expect(recovery.projectId).toBe("project-1");
    expect(recovery.maps).toEqual(previous.maps);
    expect(recovery.readbackRecovery).toEqual({ projectId: "project-1", receipt, error });
    expect(mapReadbackIsReconciled("project-1", 7, recovery.readbackRecovery!)).toBe(true);
    expect(mapReadbackIsReconciled("project-1", 6, recovery.readbackRecovery!)).toBe(false);
    expect(mapReadbackIsReconciled("different-project", 7, recovery.readbackRecovery!)).toBe(false);
  });

  it("keeps cancellation visible when a late running status arrives", () => {
    const cancelling = {
      id: "job-1",
      label: "Choosing project",
      progress: 18,
      state: "cancelling" as const,
    };
    expect(mergeActiveProjectJobStatus(cancelling, {
      jobId: "job-1",
      progress: 18,
      state: "running",
    })).toEqual(cancelling);
    expect(mergeActiveProjectJobStatus({ ...cancelling, state: "running" }, {
      jobId: "job-1",
      progress: 42,
      state: "running",
    })).toEqual({ ...cancelling, progress: 42, state: "running" });
  });
});
