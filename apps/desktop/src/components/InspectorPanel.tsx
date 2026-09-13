import { Icon } from "./Icons";

interface InspectorPanelProps {
  projectName: string;
  projectReady: boolean;
}

export function InspectorPanel({ projectName, projectReady }: InspectorPanelProps) {
  return (
    <aside className="inspector-panel" aria-label="Inspector">
      <div className="panel-title-row inspector-title"><h2>Inspector</h2><div className="inspector-actions"><button className="icon-button" type="button" disabled aria-label="Pin inspector" title="Inspector pinning is not available yet"><Icon name="pin" size={18} /></button><button className="icon-button" type="button" disabled aria-label="Close inspector" title="Inspector is required for the empty workspace"><Icon name="close" size={19} /></button></div></div>
      <div className="inspector-content">
        <div className="inspector-empty-icon"><Icon name="document" size={48} strokeWidth={1.25} /></div>
        <h3>{projectReady ? "Blank floor ready" : "No floor plan loaded"}</h3>
        <p>{projectReady ? `Project “${projectName}” is ready for the next map command.` : "Start by importing an image or PDF, then calibrate its scale."}</p>
        {!projectReady && <NextSteps />}
        {projectReady && <ReadySteps />}
        <CapabilityNotice />
        <ShortcutList />
      </div>
    </aside>
  );
}

function NextSteps() {
  const steps = [
    ["1", "Import a floor plan", "Use an image (PNG, JPG) or PDF."],
    ["2", "Calibrate scale", "Set a known distance to establish real-world measurements."],
    ["3", "Configure floors (optional)", "Add additional floors for multi-level sites."],
    ["4", "Start a survey", "Use a supported collector to capture data or plan your design."],
  ];
  return <div className="next-steps"><h4>Next steps</h4>{steps.map(([number, title, detail]) => <div className="step" key={number}><span className="step-number">{number}</span><div><strong>{title}</strong><p>{detail}</p></div></div>)}</div>;
}

function ReadySteps() {
  return <div className="next-steps ready-steps"><h4>Next steps</h4><div className="step"><span className="step-number">1</span><div><strong>Import a floor plan</strong><p>Floor geometry commands will attach to this project when available.</p></div></div><div className="step"><span className="step-number">2</span><div><strong>Configure a survey</strong><p>Capture support is negotiated by the connected collector.</p></div></div></div>;
}

function CapabilityNotice() {
  return <div className="capability-notice" role="status"><span className="notice-icon"><Icon name="triangle" size={20} /></span><div><strong>Live capture unavailable</strong><p>No collector supports scanning in this browser preview.</p><button type="button" className="text-button" disabled>Learn more <span aria-hidden="true">→</span></button></div></div>;
}

function ShortcutList() {
  const shortcuts = [["Command palette", "⌘K"], ["Select tool", "V"], ["Measure tool", "M"], ["Add access point", "A"], ["Add note", "N"], ["Survey path", "P"], ["Zoom", "Z"], ["Pan", "H"]];
  return <div className="shortcut-list"><div className="shortcut-heading"><h4>Keyboard shortcuts</h4><Icon name="question" size={17} /></div>{shortcuts.map(([label, key]) => <div className="shortcut-row" key={label}><span>{label}</span><kbd>{key}</kbd></div>)}</div>;
}
