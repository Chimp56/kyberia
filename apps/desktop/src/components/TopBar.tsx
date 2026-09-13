import type { RefObject } from "react";
import { commandPaletteShortcut } from "../lib/shortcuts";
import { Icon, LogoMark } from "./Icons";

interface TopBarProps {
  projectName: string;
  onOpenPalette: () => void;
  onNewProject: () => void;
  onOpenProject: () => void;
  onToggleInspector: () => void;
  inspectorOpen: boolean;
  paletteButtonRef: RefObject<HTMLButtonElement | null>;
}

export function TopBar({ projectName, onOpenPalette, onNewProject, onOpenProject, onToggleInspector, inspectorOpen, paletteButtonRef }: TopBarProps) {
  return (
    <header className="topbar">
      <div className="brand" aria-label="RF Atlas home">
        <LogoMark />
        <span className="brand-name">Kyberia</span>
      </div>
      <div className="topbar-divider" />
      <button className="project-name" type="button" onClick={onNewProject} aria-label="Create a new project">
        <span>{projectName}</span><Icon name="chevron-down" size={15} />
      </button>
      <div className="topbar-actions">
        <button ref={paletteButtonRef} className="palette-trigger" type="button" onClick={onOpenPalette} aria-label={`Open command palette (${commandPaletteShortcut})`}>
          <span>Command palette</span><kbd>{commandPaletteShortcut}</kbd>
        </button>
        <button className="mobile-inspector-toggle" type="button" onClick={onToggleInspector} aria-label={inspectorOpen ? "Close inspector" : "Open inspector"} aria-expanded={inspectorOpen}><Icon name="layers" size={18} /></button>
        <button className="top-action" type="button" disabled aria-label="Save project"><Icon name="save" size={18} /><span>Save</span></button>
        <span className="action-divider" />
        <button className="top-action" type="button" onClick={onOpenProject} aria-label="Open project"><Icon name="folder" size={18} /><span>Open</span></button>
        <span className="action-divider" />
        <button className="top-action project-menu" type="button" onClick={onNewProject}><span>Project</span><Icon name="chevron-down" size={14} /></button>
        <span className="action-divider" />
        <button className="top-action" type="button" disabled aria-label="Settings"><Icon name="settings" size={18} /><span>Settings</span></button>
      </div>
    </header>
  );
}
