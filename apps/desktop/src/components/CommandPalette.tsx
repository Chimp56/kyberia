import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { clampCommandSelection, moveCommandSelection } from "../lib/command-palette";
import { commandPaletteShortcut, toolShortcuts } from "../lib/shortcuts";
import { Icon } from "./Icons";
import type { ToolId } from "./ToolRail";

interface CommandPaletteProps {
  onClose: () => void;
  onSelectTool: (tool: ToolId) => void;
  onNewProject: () => void;
  onOpenProject: () => void;
  onImport: () => void;
}

interface Command {
  id: string;
  label: string;
  hint: string;
  tool?: ToolId;
  action?: "new" | "open" | "import";
}

const commands: Command[] = [
  { id: "new", label: "New project", hint: "Project", action: "new" },
  { id: "open", label: "Open project", hint: "Project", action: "open" },
  { id: "import", label: "Import floor plan", hint: "Project", action: "import" },
  { id: "select", label: "Select tool", hint: `Tool · ${toolShortcuts.v.toUpperCase()}`, tool: "select" },
  { id: "measure", label: "Measure tool", hint: `Tool · ${toolShortcuts.m.toUpperCase()}`, tool: "measure" },
  { id: "pan", label: "Pan canvas", hint: `Tool · ${toolShortcuts.h.toUpperCase()}`, tool: "pan" },
  { id: "zoom", label: "Zoom canvas", hint: `Tool · ${toolShortcuts.z.toUpperCase()}`, tool: "zoom" },
];

export function CommandPalette({ onClose, onSelectTool, onNewProject, onOpenProject, onImport }: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  const filtered = commands.filter((command) => command.label.toLowerCase().includes(query.toLowerCase()));

  useEffect(() => {
    previousFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    inputRef.current?.focus();
    return () => previousFocus.current?.focus();
  }, []);

  useEffect(() => {
    setActiveIndex((current) => clampCommandSelection(filtered.length, current));
  }, [filtered.length]);

  const runCommand = (command: Command | undefined) => {
    if (!command) return;
    if (command.action === "new") onNewProject();
    else if (command.action === "open") onOpenProject();
    else if (command.action === "import") onImport();
    else if (command.tool) onSelectTool(command.tool);
    onClose();
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      setActiveIndex((current) => moveCommandSelection(filtered.length, current, event.key === "ArrowDown" ? 1 : -1));
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      runCommand(filtered[activeIndex]);
      return;
    }
    if (event.key === "Tab") {
      event.preventDefault();
      inputRef.current?.focus();
    }
  };

  return <div className="palette-backdrop" role="presentation" onMouseDown={onClose}>
    <section className="command-palette" role="dialog" aria-modal="true" aria-label="Command palette" onMouseDown={(event) => event.stopPropagation()} onKeyDown={handleKeyDown}>
      <div className="palette-search"><Icon name="search" size={19} /><input ref={inputRef} role="combobox" aria-autocomplete="list" aria-controls="command-list" aria-expanded="true" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search commands" aria-label="Search commands" aria-activedescendant={activeIndex >= 0 ? `command-${filtered[activeIndex]?.id}` : undefined} /></div>
      <div className="command-list" id="command-list" role="listbox" aria-label="Commands">
        {filtered.length ? filtered.map((command, index) => <div id={`command-${command.id}`} role="option" aria-selected={index === activeIndex} className={index === activeIndex ? "is-active" : ""} key={command.id} onMouseEnter={() => setActiveIndex(index)} onMouseDown={(event) => event.preventDefault()} onClick={() => runCommand(command)}><span>{command.label}</span><small>{command.hint}</small></div>) : <p className="no-commands">No matching commands</p>}
      </div>
      <div className="palette-footer"><span>↑↓ Navigate</span><span>↵ Run</span><span>Esc Close</span><span className="palette-platform-shortcut">{commandPaletteShortcut}</span></div>
    </section>
  </div>;
}
