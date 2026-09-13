import { Icon } from "./Icons";

interface LayerPanelProps {
  visibility: Record<string, boolean>;
  onToggle: (id: string) => void;
}

const layers = [
  { id: "floor-plan", name: "Floor plan", swatch: "floor" },
  { id: "observations", name: "Observations", swatch: "observations" },
  { id: "prediction", name: "Prediction", swatch: "prediction" },
  { id: "requirements", name: "Requirements", swatch: "requirements" },
];

export function LayerPanel({ visibility, onToggle }: LayerPanelProps) {
  return (
    <aside className="layer-panel" aria-label="Map layers">
      <div className="panel-title-row"><h2>Layers</h2><button className="icon-button" type="button" aria-label="Add layer"><Icon name="plus" size={19} /></button></div>
      <div className="layer-list">
        {layers.map((layer, index) => (
          <div className={`layer-row ${index === 0 ? "is-active" : ""}`} key={layer.id}>
            <span className={`layer-swatch ${layer.swatch}`} aria-hidden="true" />
            <span className="layer-name">{layer.name}</span>
            <button className="layer-visibility" type="button" onClick={() => onToggle(layer.id)} aria-label={`${visibility[layer.id] ? "Hide" : "Show"} ${layer.name}`} aria-pressed={visibility[layer.id]}>
              <Icon name={visibility[layer.id] ? "eye" : "eye-off"} size={18} />
            </button>
            <button className="layer-more" type="button" aria-label={`${layer.name} options`}><Icon name="more" size={17} /></button>
          </div>
        ))}
      </div>
    </aside>
  );
}
