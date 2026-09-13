import { describe, expect, it } from "vitest";
import { assertResponse, IPC_SCHEMA, normalizeIpcError } from "./contracts";

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
});
