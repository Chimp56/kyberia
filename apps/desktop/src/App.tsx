import { useCallback, useEffect, useRef, useState } from "react";
import { CanvasStage } from "./components/CanvasStage";
import { CommandPalette } from "./components/CommandPalette";
import { InspectorPanel } from "./components/InspectorPanel";
import { LayerPanel } from "./components/LayerPanel";
import { StatusBar } from "./components/StatusBar";
import { ToolRail, type ToolId } from "./components/ToolRail";
import { TopBar } from "./components/TopBar";
import { useProjectSession } from "./lib/useProjectSession";
import { toolShortcuts } from "./lib/shortcuts";

export function App() {
  const session = useProjectSession();
  const { state } = session;
  const [inspectorOpen, setInspectorOpen] = useState(false);
  const paletteTriggerRef = useRef<HTMLButtonElement>(null);
  const projectReady = state.projectState === "baseline_only" || state.projectState === "materialized_current";
  const closePalette = useCallback(() => {
    session.setPaletteOpen(false);
    requestAnimationFrame(() => paletteTriggerRef.current?.focus());
  }, [session.setPaletteOpen]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const commandKey = event.metaKey || event.ctrlKey;
      if (commandKey && event.key.toLowerCase() === "k") {
        event.preventDefault();
        session.setPaletteOpen(!state.commandPaletteOpen);
        return;
      }
      if (event.key === "Escape" && state.commandPaletteOpen) {
        closePalette();
        return;
      }
      if (commandKey || event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement || (event.target instanceof HTMLElement && event.target.isContentEditable)) return;
      const tool = toolShortcuts[event.key.toLowerCase() as keyof typeof toolShortcuts] as ToolId | undefined;
      if (tool) {
        event.preventDefault();
        session.selectTool(tool);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [closePalette, session.selectTool, session.setPaletteOpen, state.commandPaletteOpen]);

  return <div className="app-shell">
    <TopBar projectName={state.projectName} paletteButtonRef={paletteTriggerRef} onOpenPalette={() => session.setPaletteOpen(true)} onNewProject={() => void session.createBlankProject()} onOpenProject={() => void session.openProject()} onToggleInspector={() => setInspectorOpen((current) => !current)} inspectorOpen={inspectorOpen} />
    <div className="workspace">
      <ToolRail selected={state.selectedTool as ToolId} onSelect={session.selectTool} />
      <LayerPanel visibility={state.layerVisibility} onToggle={session.toggleLayer} />
      <main className="map-region"><CanvasStage phase={state.phase} errorMessage={state.error?.message} onImport={session.importFloorPlan} onNewBlank={() => void session.createBlankProject()} onRetry={() => void session.retry()} calibrated={state.calibrated} /></main>
      <InspectorPanel projectName={state.projectName} projectReady={projectReady} isMobileOpen={inspectorOpen} />
    </div>
    <StatusBar phase={state.phase} projectName={state.projectName} />
    {state.commandPaletteOpen && <CommandPalette onClose={closePalette} onSelectTool={session.selectTool} onNewProject={() => void session.createBlankProject()} onOpenProject={() => void session.openProject()} onImport={session.importFloorPlan} />}
  </div>;
}
