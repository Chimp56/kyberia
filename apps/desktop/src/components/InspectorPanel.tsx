import { useState } from "react";
import { Icon } from "./Icons";
import { commandPaletteShortcut } from "../lib/shortcuts";
import type { CalibrateMapRequest, IpcErrorPayload, MapSummary } from "../lib/contracts";

type CalibrationInput = Pick<CalibrateMapRequest,
  "mapId" | "firstXPixels" | "firstYPixels" | "secondXPixels" | "secondYPixels" | "knownDistanceMeters">;

interface InspectorPanelProps {
  projectName: string;
  projectReady: boolean;
  isMobileOpen: boolean;
  maps: MapSummary[];
  error: IpcErrorPayload | null;
  onImport: () => void;
  onCalibrate: (input: CalibrationInput) => void;
}

export function InspectorPanel({ projectName, projectReady, isMobileOpen, maps, error, onImport, onCalibrate }: InspectorPanelProps) {
  return (
    <aside className={`inspector-panel ${isMobileOpen ? "is-open" : ""}`} aria-label="Inspector">
      <div className="panel-title-row inspector-title"><h2>Inspector</h2><div className="inspector-actions"><button className="icon-button" type="button" disabled aria-label="Pin inspector" title="Inspector pinning is not available yet"><Icon name="pin" size={18} /></button><button className="icon-button" type="button" disabled aria-label="Close inspector" title="Inspector is required for the empty workspace"><Icon name="close" size={19} /></button></div></div>
      <div className="inspector-content">
        <div className="inspector-empty-icon"><Icon name="document" size={48} strokeWidth={1.25} /></div>
        <h3>{maps.length > 0 ? "Map metadata" : projectReady ? "Floor ready" : "No project loaded"}</h3>
        <p>{maps.length > 0 ? `Project “${projectName}” contains ${maps.length} imported PNG map${maps.length === 1 ? "" : "s"}.` : projectReady ? `Project “${projectName}” has a canonical floor ready for a map.` : "Create or open a project, then import a PNG floor plan."}</p>
        {error && projectReady && <p className="state-remediation" role="status">A map operation committed, but its canonical view needs to be queried again: {error.message}</p>}
        {!projectReady && <NextSteps />}
        {projectReady && <ReadySteps onImport={onImport} />}
        {maps.map((map) => <MapCalibrationForm key={map.mapId} map={map} onCalibrate={onCalibrate} />)}
        <CapabilityNotice />
        <ShortcutList />
      </div>
    </aside>
  );
}

function NextSteps() {
  const steps = [
    ["1", "Import a floor plan", "The current workflow admits bounded PNG containers only."],
    ["2", "Calibrate scale", "Enter two distinct pixel controls and a known distance."],
    ["3", "View the map", "Raster decoding and preview are not enabled in this workflow."],
    ["4", "Start a survey", "Use a supported collector when one is connected."],
  ];
  return <div className="next-steps"><h4>Next steps</h4>{steps.map(([number, title, detail]) => <div className="step" key={number}><span className="step-number">{number}</span><div><strong>{title}</strong><p>{detail}</p></div></div>)}</div>;
}

function ReadySteps({ onImport }: { onImport: () => void }) {
  return <div className="next-steps ready-steps"><h4>Next steps</h4><div className="step"><span className="step-number">1</span><div><strong>Import a PNG plan</strong><p>Choose a bounded PNG from the native file selector.</p><button type="button" className="text-button" onClick={onImport}>Choose PNG</button></div></div><div className="step"><span className="step-number">2</span><div><strong>Configure a survey</strong><p>Capture support is negotiated by the connected collector.</p></div></div></div>;
}

function MapCalibrationForm({ map, onCalibrate }: { map: MapSummary; onCalibrate: (input: CalibrationInput) => void }) {
  const [firstX, setFirstX] = useState("0");
  const [firstY, setFirstY] = useState("0");
  const [secondX, setSecondX] = useState(String(Math.min(100, map.width - 1)));
  const [secondY, setSecondY] = useState("0");
  const [distance, setDistance] = useState("10");
  const fields = [firstX, firstY, secondX, secondY, distance];
  const set = [setFirstX, setFirstY, setSecondX, setSecondY, setDistance];
  return <form className="next-steps calibration-form" onSubmit={(event) => {
    event.preventDefault();
    const values = fields.map(Number);
    if (values.every(Number.isFinite)) onCalibrate({
      mapId: map.mapId,
      firstXPixels: values[0], firstYPixels: values[1],
      secondXPixels: values[2], secondYPixels: values[3],
      knownDistanceMeters: values[4],
    });
  }}>
    <h4>{map.calibrated ? "Recalibrate two points" : "Calibrate scale with two points"}</h4>
    <p>{map.name} · controls must be inside the {map.width} × {map.height} image bounds. No image pixels are sent to the UI.</p>
    <div className="calibration-input-grid">
      {fields.map((value, index) => <label key={index}>{["First X (px)", "First Y (px)", "Second X (px)", "Second Y (px)", "Known distance (m)"][index]}<input type="number" step="any" required min={index === 4 ? Number.MIN_VALUE : 0} value={value} onChange={(event) => set[index](event.target.value)} /></label>)}
    </div>
    {map.calibrated && map.metersPerPixel !== null && <p>Persisted scale: {map.metersPerPixel.toPrecision(5)} m/pixel</p>}
    <button className="secondary-button" type="submit">Save calibration</button>
  </form>;
}

function CapabilityNotice() {
  return <div className="capability-notice" role="status"><span className="notice-icon"><Icon name="triangle" size={20} /></span><div><strong>Live capture unavailable</strong><p>No collector supports scanning in this browser preview.</p><button type="button" className="text-button" disabled>Learn more <span aria-hidden="true">→</span></button></div></div>;
}

function ShortcutList() {
  const shortcuts = [["Command palette", commandPaletteShortcut], ["Select tool", "V"], ["Measure tool", "M"], ["Add access point", "A"], ["Add note", "N"], ["Survey path", "P"], ["Zone", "Q"], ["Zoom", "Z"], ["Pan", "H"]];
  return <div className="shortcut-list"><div className="shortcut-heading"><h4>Keyboard shortcuts</h4><Icon name="question" size={17} /></div>{shortcuts.map(([label, key]) => <div className="shortcut-row" key={label}><span>{label}</span><kbd>{key}</kbd></div>)}</div>;
}
