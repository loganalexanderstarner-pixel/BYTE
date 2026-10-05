import { contextLabel } from "../lib/format";
import { displayName } from "../lib/models";
import { useStore } from "../state/store";
import { osText } from "../lib/platform";

export function EngineBadge() {
  const engine = useStore((s) => s.engine);
  const models = useStore((s) => s.models);
  const openSettings = useStore((s) => s.openSettings);
  const name = (key: string) => displayName(models, key);

  let cls = "pill";
  let dot = "";
  let label = "";
  let title = "";
  switch (engine.state) {
    case "ready":
      cls += " ok";
      label = `${engine.boosted ? "⚡ " : ""}${name(engine.model)} · ${contextLabel(engine.context)}`;
      title = osText(`Running on this Mac with a ${engine.context.toLocaleString()}-token context window.${engine.boosted ? " Speed boost is on." : ""}`);
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
