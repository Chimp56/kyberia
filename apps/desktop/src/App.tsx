import { useEffect } from "react";
import { CanvasStage } from "./components/CanvasStage";
import { CommandPalette } from "./components/CommandPalette";
import { InspectorPanel } from "./components/InspectorPanel";
import { LayerPanel } from "./components/LayerPanel";
import { StatusBar } from "./components/StatusBar";
import { ToolRail, type ToolId } from "./components/ToolRail";
import { TopBar } from "./components/TopBar";
import { useProjectSession } from "./lib/useProjectSession";

export function App() {
  const session = useProjectSession();
  const { state } = session;
  const projectReady = state.projectState === "baseline_only" || state.projectState === "materialized_current";

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const commandKey = event.metaKey || event.ctrlKey;
      if (commandKey && event.key.toLowerCase() === "k") {
        event.preventDefault();
        session.setPaletteOpen(!state.commandPaletteOpen);
        return;
      }
      if (event.key === "Escape" && state.commandPaletteOpen) {
        session.setPaletteOpen(false);
        return;
      }
      if (commandKey || event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) return;
      const shortcuts: Record<string, ToolId> = { v: "select", m: "measure", a: "access-point", n: "note", p: "survey", h: "pan", z: "zoom" };
      const tool = shortcuts[event.key.toLowerCase()];
      if (tool) {
        event.preventDefault();
        session.selectTool(tool);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [session, state.commandPaletteOpen]);

  return <div className="app-shell">
    <TopBar projectName={state.projectName} onOpenPalette={() => session.setPaletteOpen(true)} onNewProject={() => void session.createBlankProject()} />
    <div className="workspace">
      <ToolRail selected={state.selectedTool as ToolId} onSelect={session.selectTool} />
      <LayerPanel visibility={state.layerVisibility} onToggle={session.toggleLayer} />
      <main className="map-region"><CanvasStage phase={state.phase} errorMessage={state.error?.message} onImport={session.importFloorPlan} onNewBlank={() => void session.createBlankProject()} onRetry={() => void session.retry()} /></main>
      <InspectorPanel projectName={state.projectName} projectReady={projectReady} />
    </div>
    <StatusBar phase={state.phase} projectName={state.projectName} />
    {state.commandPaletteOpen && <CommandPalette onClose={() => session.setPaletteOpen(false)} onSelectTool={session.selectTool} onNewProject={() => void session.createBlankProject()} onImport={session.importFloorPlan} />}
  </div>;
}
