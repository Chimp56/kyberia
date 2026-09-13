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
  { id: "new", label: "New blank floor", hint: "Project", action: "new" },
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
  const commandRefs = useRef<Array<HTMLButtonElement | null>>([]);
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
    if (event.key !== "Tab") return;
    const focusable = [inputRef.current, ...commandRefs.current.slice(0, filtered.length)].filter((item): item is HTMLInputElement | HTMLButtonElement => item !== null);
    if (focusable.length === 0) return;
    const current = focusable.indexOf(document.activeElement as HTMLInputElement | HTMLButtonElement);
    const next = event.shiftKey
      ? (current <= 0 ? focusable.length - 1 : current - 1)
      : (current === focusable.length - 1 ? 0 : current + 1);
    event.preventDefault();
    focusable[next]?.focus();
  };

  return <div className="palette-backdrop" role="presentation" onMouseDown={onClose}>
    <section className="command-palette" role="dialog" aria-modal="true" aria-label="Command palette" onMouseDown={(event) => event.stopPropagation()} onKeyDown={handleKeyDown}>
      <div className="palette-search"><Icon name="search" size={19} /><input ref={inputRef} value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search commands" aria-label="Search commands" aria-activedescendant={activeIndex >= 0 ? `command-${filtered[activeIndex]?.id}` : undefined} /></div>
      <div className="command-list">
        {filtered.length ? filtered.map((command, index) => <button ref={(element) => { commandRefs.current[index] = element; }} id={`command-${command.id}`} className={index === activeIndex ? "is-active" : ""} type="button" key={command.id} onMouseEnter={() => setActiveIndex(index)} onClick={() => runCommand(command)}><span>{command.label}</span><small>{command.hint}</small></button>) : <p className="no-commands">No matching commands</p>}
      </div>
      <div className="palette-footer"><span>↑↓ Navigate</span><span>↵ Run</span><span>Esc Close</span><span className="palette-platform-shortcut">{commandPaletteShortcut}</span></div>
    </section>
  </div>;
}
