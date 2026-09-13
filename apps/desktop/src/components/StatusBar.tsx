import { Icon } from "./Icons";

interface StatusBarProps {
  phase: "idle" | "loading" | "ready" | "error" | "unsupported";
  projectName: string;
}

export function StatusBar({ phase, projectName }: StatusBarProps) {
  const captureUnavailable = phase !== "loading";
  return <footer className="statusbar"><div className="status-survey"><span className="status-radio" /> <span>No active survey</span></div><div className="status-alert"><Icon name="triangle" size={18} /><span>{captureUnavailable ? "Live capture unavailable" : "Checking collector capabilities"}</span><Icon name={phase === "unsupported" ? "chevron-up" : "chevron-down"} size={14} /></div><div className="status-spacer" /><div className="status-coordinates"><span className="crosshair">⌾</span><span>X: —</span><span>Y: —</span></div><div className="status-divider" /><div className="status-zoom">100% <Icon name="chevron-down" size={13} /></div><div className="status-divider" /><span className="status-crs">EPSG:3857 (Web Mercator)</span><div className="status-divider" /><span className="status-project">{projectName}</span><span className="status-save">Not saved</span><Icon name="cloud" size={17} /></footer>;
}
