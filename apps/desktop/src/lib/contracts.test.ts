import { describe, expect, it } from "vitest";
import { assertJobCancelResponse, assertJobStatusResponse, assertMapMutationResponse, assertMapSourceSelectionResponse, assertOpenProjectSelectionResponse, assertResponse, IPC_SCHEMA, isIpcErrorPayload, normalizeIpcError } from "./contracts";

const jobId = "123e4567-e89b-42d3-a456-426614174000";

describe("desktop IPC contract", () => {
  it("rejects a response from an unversioned adapter", () => {
    expect(() => assertResponse({ state: "no_project" })).toThrow();
  });

  it("preserves structured error remediation", () => {
    const error = normalizeIpcError({
      schema: IPC_SCHEMA,
      code: "capability_unavailable",
      message: "Not observable on this adapter.",
      remediation: "Pair a Linux sensor.",
      retryable: false,
    });
    expect(error.code).toBe("capability_unavailable");
    expect(error.remediation).toBe("Pair a Linux sensor.");
  });

  it("rejects malformed same-version success and error fields", () => {
    expect(() => assertResponse({ schema: IPC_SCHEMA, state: "no_project", project: null, capabilities: [{ id: "capture", label: "Capture", state: "unavailable", detail: "No collector", remediation: 3 }] })).toThrow(/malformed/);
    expect(isIpcErrorPayload({ schema: IPC_SCHEMA, code: "storage", message: "Nope", retryable: "yes" })).toBe(false);
    expect(normalizeIpcError({ schema: IPC_SCHEMA, code: "storage", message: "Nope", retryable: "yes" }).code).toBe("invalid_response");
  });

  it("scrubs local paths before structured errors reach rendered copy", () => {
    const error = normalizeIpcError({ schema: IPC_SCHEMA, code: "storage", message: "Could not open /Users/vincent/secret.rfatlas", remediation: "Check /Users/vincent", retryable: true });
    expect(error.message).not.toContain("/Users/vincent");
    expect(error.remediation).not.toContain("/Users/vincent");
  });

  it("scrubs Windows UNC paths before rendered errors", () => {
    const error = normalizeIpcError({ schema: IPC_SCHEMA, code: "storage", message: "Could not open \\\\server\\share\\rf atlas.rfatlas", remediation: "Check \\\\server\\share", retryable: true });
    expect(error.message).not.toContain("server");
    expect(error.remediation).not.toContain("server");
  });

  it("validates nested native selection grants", () => {
    expect(assertOpenProjectSelectionResponse({ schema: IPC_SCHEMA, selection: null }).selection).toBeNull();
    expect(() => assertOpenProjectSelectionResponse({ schema: IPC_SCHEMA, selection: { grantId: "g", displayName: "Plan.rfatlas", kind: "open", path: "/private/user/secret" } })).toThrow(/malformed/);
    expect(() => assertOpenProjectSelectionResponse({ schema: IPC_SCHEMA, selection: { grantId: "", displayName: "Plan.rfatlas", kind: "open" } })).toThrow(/malformed/);
  });

  it("admits only bounded opaque PNG grants and rejects path or pixel fields", () => {
    expect(assertMapSourceSelectionResponse({
      schema: IPC_SCHEMA,
      selection: { grantId: jobId, displayName: "Floor.png", byteLength: 1024, kind: "png" },
    }).selection?.displayName).toBe("Floor.png");
    expect(assertMapSourceSelectionResponse({ schema: IPC_SCHEMA, selection: null }).selection).toBeNull();
    expect(() => assertMapSourceSelectionResponse({
      schema: IPC_SCHEMA,
      selection: { grantId: jobId, displayName: "Floor.png", byteLength: 1024, kind: "png", path: "/private/Floor.png" },
    })).toThrow(/invalid PNG selection/);
    expect(() => assertMapSourceSelectionResponse({
      schema: IPC_SCHEMA,
      selection: { grantId: jobId, displayName: "Floor.png", byteLength: 32 * 1024 * 1024 + 1, kind: "png" },
    })).toThrow(/invalid PNG selection/);
  });

  it("accepts receipt-first map responses without leaking source or pixel fields", () => {
    const receipt = {
      schema: IPC_SCHEMA,
      state: "committed",
      operationId: jobId,
      projectRevision: 2,
      contentHash: "a".repeat(64),
      current: null,
      readbackError: {
        schema: IPC_SCHEMA,
        code: "storage",
        message: "The durable map commit needs readback.",
        retryable: true,
      },
    };
    expect(assertMapMutationResponse(receipt).state).toBe("committed");
    expect(() => assertMapMutationResponse({ ...receipt, sourcePath: "/private/Floor.png" })).toThrow(/invalid map mutation receipt/);
    expect(() => assertMapMutationResponse({ ...receipt, pixels: [0, 1, 2] })).toThrow(/invalid map mutation receipt/);
  });

  it("strictly validates bounded job progress and cancellation acknowledgements", () => {
    expect(assertJobStatusResponse({ schema: IPC_SCHEMA, jobId, state: "running", progress: 42 }).progress).toBe(42);
    expect(assertJobCancelResponse({ schema: IPC_SCHEMA, jobId, state: "cancelling" }).state).toBe("cancelling");
    expect(() => assertJobStatusResponse({ schema: IPC_SCHEMA, jobId, state: "done", progress: 42 })).toThrow(/malformed/);
    expect(() => assertJobStatusResponse({ schema: IPC_SCHEMA, jobId, state: "running", progress: 101 })).toThrow(/malformed/);
    expect(() => assertJobStatusResponse({ schema: IPC_SCHEMA, jobId: "../../job", state: "running", progress: 1 })).toThrow(/malformed/);
    expect(() => assertJobCancelResponse({ schema: IPC_SCHEMA, jobId, state: "cancelling", path: "/private/project" })).toThrow(/malformed/);
  });
});
