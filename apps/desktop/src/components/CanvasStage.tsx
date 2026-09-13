import { useState } from "react";
import { Icon } from "./Icons";

interface CanvasStageProps {
  phase: "idle" | "loading" | "ready" | "error" | "unsupported";
  errorMessage?: string;
  onImport: () => void;
  onNewBlank: () => void;
  onRetry: () => void;
}

export function CanvasStage({ phase, errorMessage, onImport, onNewBlank, onRetry }: CanvasStageProps) {
  const [zoom, setZoom] = useState(100);
  return (
    <section className="canvas-stage" aria-label="Floor plan canvas">
      <div className="axis axis-top" aria-hidden="true"><span>-30</span><span>-20</span><span>-10</span><span>0</span><span>10</span><span>20</span><span>30</span><span>40</span></div>
      <div className="axis axis-left" aria-hidden="true"><span>30</span><span>20</span><span>10</span><span>0</span><span>-10</span><span>-20</span><span>-30</span></div>
      {phase === "loading" ? <LoadingState /> : phase === "error" ? <ErrorState message={errorMessage} onRetry={onRetry} /> : phase === "unsupported" ? <UnsupportedState message={errorMessage} /> : <EmptyCanvas onImport={onImport} onNewBlank={onNewBlank} />}
      <CanvasControls zoom={zoom} onZoomChange={setZoom} />
      <div className="scale-bar" aria-label="Scale is unavailable until a floor plan is loaded"><span /><small>10 m</small></div>
    </section>
  );
}

function EmptyCanvas({ onImport, onNewBlank }: { onImport: () => void; onNewBlank: () => void }) {
  return (
    <div className="empty-canvas-card">
      <div className="empty-file-icon"><Icon name="document" size={40} strokeWidth={1.4} /></div>
      <h1>Import floor plan</h1>
      <p>Drag and drop an image or PDF here<br />or choose a file to get started.</p>
      <button className="primary-button" type="button" onClick={onImport}><Icon name="folder" size={20} />Import floor plan</button>
      <div className="or-divider"><span>or</span></div>
      <button className="secondary-button" type="button" onClick={onNewBlank}><Icon name="document" size={19} />New blank floor</button>
    </div>
  );
}

function LoadingState() {
  return <div className="canvas-state-card" role="status" aria-live="polite"><span className="loading-spinner" /><h1>Opening project</h1><p>Verifying the canonical project snapshot…</p></div>;
}

function ErrorState({ message, onRetry }: { message?: string; onRetry: () => void }) {
  return <div className="canvas-state-card error-state" role="alert"><span className="state-symbol">!</span><h1>Project unavailable</h1><p>{message ?? "The project could not be opened."}</p><button className="secondary-button" type="button" onClick={onRetry}>Try again</button></div>;
}

function UnsupportedState({ message }: { message?: string }) {
  return <div className="canvas-state-card unsupported-state" role="status"><span className="state-symbol">~</span><h1>Desktop command required</h1><p>{message ?? "This capability is not available in the browser preview."}</p></div>;
}

function CanvasControls({ zoom, onZoomChange }: { zoom: number; onZoomChange: (zoom: number) => void }) {
  return <div className="canvas-controls"><button type="button" onClick={() => onZoomChange(Math.max(25, zoom - 10))} aria-label="Zoom out">−</button><span>{zoom}%</span><button type="button" onClick={() => onZoomChange(Math.min(400, zoom + 10))} aria-label="Zoom in">+</button><button type="button" onClick={() => onZoomChange(100)} aria-label="Fit canvas"><Icon name="fullscreen" size={16} /></button></div>;
}
