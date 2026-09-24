import { useState } from "react";
import type { IpcErrorPayload } from "../lib/contracts";
import type { MapSummary } from "../lib/contracts";
import type { ActiveProjectJob, MapReadbackRecovery } from "../lib/ui-state";
import { Icon } from "./Icons";

interface CanvasStageProps {
  phase: "idle" | "loading" | "ready" | "error" | "unsupported";
  error: IpcErrorPayload | null;
  readbackRecovery: MapReadbackRecovery | null;
  activeJob: ActiveProjectJob | null;
  onImport: () => void;
  onNewProject: () => void;
  onRetry: () => void;
  onCancel: () => void;
  calibrated: boolean;
  maps: MapSummary[];
}

export function CanvasStage({ phase, error, readbackRecovery, activeJob, onImport, onNewProject, onRetry, onCancel, calibrated, maps }: CanvasStageProps) {
  const [zoom, setZoom] = useState(100);
  const map = maps[0] ?? null;
  return (
    <section className="canvas-stage" aria-label="Floor plan canvas">
      {phase === "loading" ? <LoadingState job={activeJob} onCancel={onCancel} /> : phase === "error" ? <ErrorState error={error} readbackRecovery={readbackRecovery} onRetry={onRetry} /> : phase === "unsupported" ? <UnsupportedState message={error?.message} /> : map ? <ImportedMapState map={map} onImport={onImport} /> : <EmptyCanvas onImport={onImport} onNewProject={onNewProject} />}
      <CanvasControls zoom={zoom} onZoomChange={setZoom} />
      <div className={`scale-bar ${calibrated ? "" : "is-unavailable"}`} aria-label={calibrated ? "Map scale calibration is persisted" : "Scale unavailable until a floor plan is calibrated"}><span />{calibrated ? <small>Scale calibrated</small> : <small>Scale unavailable</small>}</div>
    </section>
  );
}

function EmptyCanvas({ onImport, onNewProject }: { onImport: () => void; onNewProject: () => void }) {
  return (
    <div className="empty-canvas-card">
      <div className="empty-file-icon"><Icon name="document" size={40} strokeWidth={1.4} /></div>
      <h1>Import floor plan</h1>
      <p>Choose a PNG from the desktop file selector.<br />This workflow stores validated metadata only; it does not display a raster preview.</p>
      <button className="primary-button" type="button" onClick={() => void onImport()}><Icon name="folder" size={20} />Choose PNG floor plan</button>
      <div className="or-divider"><span>or</span></div>
      <button className="secondary-button" type="button" onClick={onNewProject}><Icon name="document" size={19} />New project</button>
    </div>
  );
}

function ImportedMapState({ map, onImport }: { map: MapSummary; onImport: () => void }) {
  return <div className="canvas-state-card" role="status" aria-label="Imported map metadata">
    <span className="state-symbol">▧</span>
    <h1>PNG map imported</h1>
    <p>{map.name}</p>
    <p>{map.width} × {map.height} pixels · raster preview unavailable</p>
    {map.calibrated && map.metersPerPixel !== null
      ? <p>Persisted scale: {map.metersPerPixel.toPrecision(5)} m/pixel</p>
      : <p>Scale not calibrated</p>}
    <button className="secondary-button" type="button" onClick={() => void onImport()}><Icon name="folder" size={18} />Import another PNG</button>
  </div>;
}

function LoadingState({ job, onCancel }: { job: ActiveProjectJob | null; onCancel: () => void }) {
  const label = job?.label ?? "Choosing project";
  return <div className="canvas-state-card" role="status" aria-live="polite"><span className="loading-spinner" /><h1>{label}</h1><p>{job ? `${job.state === "cancelling" ? "Cancelling" : "Working"} — ${job.progress}%` : "Waiting for the native project selector…"}</p>{job && <><progress className="job-progress" max={100} value={job.progress} aria-label={`${label} progress`} /><button className="secondary-button" type="button" onClick={onCancel} disabled={job.state === "cancelling"}>{job.state === "cancelling" ? "Cancelling…" : "Cancel"}</button></>}</div>;
}

function ErrorState({ error, readbackRecovery, onRetry }: { error: IpcErrorPayload | null; readbackRecovery: MapReadbackRecovery | null; onRetry: () => void }) {
  const title = readbackRecovery
    ? "Committed map change needs reconciliation"
    : error?.code === "cancelled" ? "Operation cancelled" : "Project unavailable";
  const message = readbackRecovery
    ? `Operation ${readbackRecovery.receipt.operationId} committed at revision ${readbackRecovery.receipt.projectRevision}, but its canonical view was not returned. Refresh the project view to reconcile without repeating the mutation.`
    : error?.message ?? "The project could not be opened.";
  const canRetry = readbackRecovery !== null || error?.retryable === true;
  const retryLabel = readbackRecovery ? "Refresh project view" : "Try again";
  return <div className="canvas-state-card error-state" role="alert"><span className="state-symbol">!</span><h1>{title}</h1><p>{message}</p>{!readbackRecovery && error?.remediation && <p className="state-remediation">{error.remediation}</p>}{canRetry && <button className="secondary-button" type="button" onClick={onRetry}>{retryLabel}</button>}</div>;
}

function UnsupportedState({ message }: { message?: string }) {
  return <div className="canvas-state-card unsupported-state" role="status"><span className="state-symbol">~</span><h1>Desktop command required</h1><p>{message ?? "This capability is not available in the browser preview."}</p></div>;
}

function CanvasControls({ zoom, onZoomChange }: { zoom: number; onZoomChange: (zoom: number) => void }) {
  return <div className="canvas-controls"><button type="button" onClick={() => onZoomChange(Math.max(25, zoom - 10))} aria-label="Zoom out">−</button><span>{zoom}%</span><button type="button" onClick={() => onZoomChange(Math.min(400, zoom + 10))} aria-label="Zoom in">+</button><button type="button" onClick={() => onZoomChange(100)} aria-label="Fit canvas"><Icon name="fullscreen" size={16} /></button></div>;
}
