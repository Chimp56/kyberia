import { Icon } from "./Icons";

export type ToolId = "select" | "measure" | "access-point" | "note" | "survey" | "zone" | "text" | "pan" | "zoom";

const tools: Array<{ id: ToolId; label: string; shortcut: string; icon: "arrow" | "grid" | "plus" | "document" | "layers" | "triangle" | "menu" | "pin" | "search" }> = [
  { id: "select", label: "Select", shortcut: "V", icon: "arrow" },
  { id: "measure", label: "Measure", shortcut: "M", icon: "grid" },
  { id: "access-point", label: "Add AP", shortcut: "A", icon: "plus" },
  { id: "note", label: "Add Note", shortcut: "N", icon: "document" },
  { id: "survey", label: "Survey Path", shortcut: "P", icon: "layers" },
  { id: "zone", label: "Zone", shortcut: "Q", icon: "triangle" },
  { id: "text", label: "Text", shortcut: "T", icon: "menu" },
  { id: "pan", label: "Pan", shortcut: "H", icon: "pin" },
  { id: "zoom", label: "Zoom", shortcut: "Z", icon: "search" },
];

interface ToolRailProps {
  selected: ToolId;
  onSelect: (tool: ToolId) => void;
}

export function ToolRail({ selected, onSelect }: ToolRailProps) {
  return (
    <nav className="tool-rail" aria-label="Map tools">
      {tools.map((tool, index) => (
        <div key={tool.id} className={index === 7 ? "tool-spacer" : ""}>
          <button className={`tool-button ${selected === tool.id ? "is-selected" : ""}`} type="button" onClick={() => onSelect(tool.id)} aria-label={`${tool.label} (${tool.shortcut})`} aria-pressed={selected === tool.id}>
            <Icon name={tool.icon} size={22} strokeWidth={selected === tool.id ? 1.4 : 1.55} />
            <span>{tool.label}</span>
            <kbd>{tool.shortcut}</kbd>
          </button>
        </div>
      ))}
    </nav>
  );
}
