import { describe, expect, it } from "vitest";
import { initialWorkspaceState, stateForError } from "./ui-state";

describe("workspace state semantics", () => {
  it("starts without inventing a project or measurements", () => {
    expect(initialWorkspaceState.projectState).toBe("no_project");
    expect(initialWorkspaceState.phase).toBe("idle");
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
});
