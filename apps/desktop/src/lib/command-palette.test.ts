import { describe, expect, it } from "vitest";
import { clampCommandSelection, moveCommandSelection } from "./command-palette";

describe("command palette navigation", () => {
  it("wraps arrow navigation and handles an empty result", () => {
    expect(moveCommandSelection(3, 0, -1)).toBe(2);
    expect(moveCommandSelection(3, 2, 1)).toBe(0);
    expect(moveCommandSelection(0, 0, 1)).toBe(-1);
  });

  it("clamps a selection after filtering", () => {
    expect(clampCommandSelection(2, 8)).toBe(1);
    expect(clampCommandSelection(0, 1)).toBe(-1);
  });
});
