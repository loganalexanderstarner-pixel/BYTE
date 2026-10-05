import { contextLabel } from "../lib/format";
import { displayName } from "../lib/models";
import { spaceOf, useStore, workspaceOf } from "../state/store";

export function EngineBadge() {
  const engine = useStore((s) => s.engine);
  const models = useStore((s) => s.models);
  const openSettings = useStore((s) => s.openSettings);
  const settings = useStore((s) => s.settings);
  const currentId = useStore((s) => s.currentId);
  const privateChat = useStore((s) => s.conversations.find((c) => c.id === s.currentId)?.private ?? false);
  // In the Cloud workspace the answer doesn't come from the engine, so "Engine stopped" would read like an error.
  const onCloud = !!settings?.cloudConnected && !privateChat && (currentId ? spaceOf(currentId) : workspaceOf(settings)) === "cloud";
  const name = (key: string) => displayName(models, key);

  let cls = "pill engine-badge";
  let dot = "";
  let label = "";
  let title = "";
  switch (engine.state) {
    case "ready":
      cls += " ok";
      label = `${engine.boosted ? "⚡ " : ""}${name(engine.model)} · ${contextLabel(engine.context)}`;
      title = `Running on this Mac with a ${engine.context.toLocaleString()}-token context window.${engine.boosted ? " Speed boost is on." : ""}`;
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
  if (onCloud) {
    return (
      <button className="pill engine-badge accent" onClick={() => openSettings("cloud")} title="Answers come from your BYTE cloud." style={{ cursor: "pointer" }}>
        <span className="dot" />
        BYTE Cloud
      </button>
    );
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
