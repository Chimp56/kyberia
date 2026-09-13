import { Icon, LogoMark } from "./Icons";

interface TopBarProps {
  projectName: string;
  onOpenPalette: () => void;
  onNewProject: () => void;
}

export function TopBar({ projectName, onOpenPalette, onNewProject }: TopBarProps) {
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
        <button className="palette-trigger" type="button" onClick={onOpenPalette} aria-label="Open command palette">
          <span>Command palette</span><kbd>⌘K</kbd>
        </button>
        <button className="top-action" type="button" disabled aria-label="Save project"><Icon name="save" size={18} /><span>Save</span></button>
        <span className="action-divider" />
        <button className="top-action" type="button" disabled aria-label="Open project"><Icon name="folder" size={18} /><span>Open</span></button>
        <span className="action-divider" />
        <button className="top-action project-menu" type="button" onClick={onNewProject}><span>Project</span><Icon name="chevron-down" size={14} /></button>
        <span className="action-divider" />
        <button className="top-action" type="button" disabled aria-label="Settings"><Icon name="settings" size={18} /><span>Settings</span></button>
      </div>
    </header>
  );
}
