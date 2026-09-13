export const commandPaletteShortcut = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform)
  ? "⌘K"
  : "Ctrl K";

export const toolShortcuts = {
  v: "select",
  m: "measure",
  a: "access-point",
  n: "note",
  p: "survey",
  q: "zone",
  t: "text",
  h: "pan",
  z: "zoom",
} as const;
