import { contextLabel } from "../lib/format";
import { useStore } from "../state/store";

export function EngineBadge() {
  const engine = useStore((s) => s.engine);
  const models = useStore((s) => s.models);
  const openSettings = useStore((s) => s.openSettings);
  const name = (id: string) => models.find((m) => m.id === id)?.name ?? id;

  let cls = "pill";
  let dot = "";
  let label = "";
  let title = "";
  switch (engine.state) {
    case "ready":
      cls += " ok";
      label = `${name(engine.model)} · ${contextLabel(engine.context)}`;
      title = `Running on this Mac with a ${engine.context.toLocaleString()}-token context window.`;
      break;
    case "starting":
      cls += " warn";
      dot = "pulse";
      label = `Loading ${name(engine.model)}…`;
      title = "Loading the model into memory. This takes a few seconds.";
      break;
    case "error":
      cls += " danger";
      label = "Engine problem";
      title = engine.message;
      break;
    case "noModel":
      label = "No model";
      title = "Download a model in Settings → Models.";
      break;
    default:
      label = "Engine stopped";
  }
  return (
    <button
      className={cls}
      onClick={() => openSettings(engine.state === "error" ? "engine" : "models")}
      title={title}
      style={{ cursor: "pointer" }}
    >
      <span className={`dot ${dot}`} />
      {label}
    </button>
  );
}
