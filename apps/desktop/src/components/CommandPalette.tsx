import { useEffect, useRef, useState } from "react";
import { Icon } from "./Icons";
import type { ToolId } from "./ToolRail";

interface CommandPaletteProps {
  onClose: () => void;
  onSelectTool: (tool: ToolId) => void;
  onNewProject: () => void;
  onImport: () => void;
}

const commands: Array<{ id: string; label: string; hint: string; tool?: ToolId }> = [
  { id: "new", label: "New blank floor", hint: "Project", },
  { id: "import", label: "Import floor plan", hint: "Project" },
  { id: "select", label: "Select tool", hint: "Tool · V", tool: "select" },
  { id: "measure", label: "Measure tool", hint: "Tool · M", tool: "measure" },
  { id: "pan", label: "Pan canvas", hint: "Tool · H", tool: "pan" },
  { id: "zoom", label: "Zoom canvas", hint: "Tool · Z", tool: "zoom" },
];

export function CommandPalette({ onClose, onSelectTool, onNewProject, onImport }: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const filtered = commands.filter((command) => command.label.toLowerCase().includes(query.toLowerCase()));
  useEffect(() => { inputRef.current?.focus(); }, []);
  return <div className="palette-backdrop" role="presentation" onMouseDown={onClose}><section className="command-palette" role="dialog" aria-modal="true" aria-label="Command palette" onMouseDown={(event) => event.stopPropagation()}><div className="palette-search"><Icon name="search" size={19} /><input ref={inputRef} value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search commands" aria-label="Search commands" /></div><div className="command-list">{filtered.length ? filtered.map((command) => <button type="button" key={command.id} onClick={() => { if (command.id === "new") onNewProject(); else if (command.id === "import") onImport(); else if (command.tool) onSelectTool(command.tool); onClose(); }}><span>{command.label}</span><small>{command.hint}</small></button>) : <p className="no-commands">No matching commands</p>}</div><div className="palette-footer"><span>↑↓ Navigate</span><span>↵ Run</span><span>Esc Close</span></div></section></div>;
}
