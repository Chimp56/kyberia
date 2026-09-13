import { describe, expect, it } from "vitest";
import { createRequestGate, initialWorkspaceState, stateForError } from "./ui-state";

describe("workspace state semantics", () => {
  it("starts without inventing a project or measurements", () => {
    expect(initialWorkspaceState.projectState).toBe("no_project");
    expect(initialWorkspaceState.phase).toBe("idle");
    expect(initialWorkspaceState.activeJob).toBeNull();
    expect(initialWorkspaceState.layerVisibility.prediction).toBe(false);
  });

  it("makes unsupported capability explicit", () => {
    const state = stateForError({
      schema: "kyberia.desktop-ipc/1",
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
      schema: "kyberia.desktop-ipc/1",
      code: "capability_unavailable",
      message: "Import is unavailable.",
      retryable: false,
    }, current);
    expect(next.projectState).toBe("baseline_only");
    expect(next.projectName).toBe("Office");
  });
});
