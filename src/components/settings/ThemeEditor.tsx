import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, Download, Trash2, Upload, X } from "lucide-react";
import { useRef, useState } from "react";

import { THEMES } from "../../design/themes";
import { api, errorText, inTauri } from "../../lib/api";
import { exportTheme, importTheme, KEYS, LABELS, parseTheme, themeWarnings, upsert, type CustomTheme, type ThemeColors } from "../../lib/customTheme";
import { useStore } from "../../state/store";

const START: ThemeColors = { bg: "#0f1420", panel: "#171d2c", border: "#2a3247", text: "#e7ebf3", accent: "#4c8dff" };

/** Your saved themes, from settings (only valid ones). */
export function customThemes(raw: unknown[] | undefined): CustomTheme[] {
  return (raw ?? []).map(parseTheme).filter((t): t is CustomTheme => !!t);
}

/** "Make your own" theme: five colors, a live preview, a readability check, import and export. */
export function ThemeEditor({ editing, onClose }: { editing: CustomTheme | null; onClose: () => void }) {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const [name, setName] = useState(editing?.name ?? "My theme");
  const [colors, setColors] = useState<ThemeColors>(editing?.colors ?? START);
  const [error, setError] = useState<string | null>(null);
  const file = useRef<HTMLInputElement>(null);
  const warnings = themeWarnings(colors);
  const list = customThemes(settings?.customThemes);

  const startFrom = (id: string) => {
    const t = THEMES.find((x) => x.id === id);
    if (!t) return;
    const light = !!t.light;
    setColors({ bg: t.swatch[0], panel: t.swatch[1], border: light ? "#d9dce3" : "#2a3247", text: light ? "#1d1f24" : "#e7ebf3", accent: t.swatch[2] });
  };

  const saveTheme = async () => {
    setError(null);
    const t = parseTheme({ name, colors });
    if (!t) return setError("Give the theme a name, and use hex colors like #1a2b3c.");
    try {
      const next = upsert(editing && editing.id !== t.id ? list.filter((x) => x.id !== editing.id) : list, t);
      await update({ customThemes: next, theme: t.id });
      onClose();
    } catch (e) {
      setError(errorText(e));
    }
  };

  const remove = async () => {
    if (!editing) return;
    await update({ customThemes: list.filter((x) => x.id !== editing.id), theme: settings?.theme === editing.id ? "midnight" : settings?.theme });
    onClose();
  };

  const exportIt = async () => {
    const t = parseTheme({ name, colors });
    if (!t) return setError("Give the theme a name first.");
    if (!inTauri) return;
    const dest = await saveDialog({ defaultPath: `${t.name}.bytetheme`, filters: [{ name: "BYTE theme", extensions: ["bytetheme"] }] }).catch(() => null);
    if (dest) await api.docSave(dest, btoa(unescape(encodeURIComponent(exportTheme(t)))));
  };

  const importIt = async (f: File | undefined) => {
    if (!f) return;
    try {
      const t = importTheme(await f.text());
      setName(t.name);
      setColors(t.colors);
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className="theme-editor">
      <div className="row" style={{ gap: 8, alignItems: "center" }}>
        <input className="text-input grow" value={name} maxLength={40} onChange={(e) => setName(e.target.value)} aria-label="Theme name" />
        <select onChange={(e) => (e.target.value ? startFrom(e.target.value) : undefined)} value="" aria-label="Start from a theme">
          <option value="">Start from…</option>
          {THEMES.filter((t) => t.id !== "system").map((t) => (
            <option key={t.id} value={t.id}>
              {t.name}
            </option>
          ))}
        </select>
        <button className="icon-btn" onClick={onClose} aria-label="Close the theme editor">
          <X size={14} />
        </button>
      </div>
      <div className="theme-editor-body">
        <div className="theme-colors">
          {KEYS.map((k) => (
            <label key={k} className="theme-color">
              <span>{LABELS[k]}</span>
              <input type="color" value={/^#[0-9a-f]{6}$/i.test(colors[k]) ? colors[k] : "#000000"} onChange={(e) => setColors({ ...colors, [k]: e.target.value })} aria-label={`${LABELS[k]} color`} />
              <input className="text-input mono" value={colors[k]} maxLength={7} onChange={(e) => setColors({ ...colors, [k]: e.target.value })} aria-label={`${LABELS[k]} hex`} />
            </label>
          ))}
        </div>
        <div className="theme-preview" style={{ background: colors.bg, color: colors.text, borderColor: colors.border }} aria-label="Preview">
          <div className="tp-panel" style={{ background: colors.panel, borderColor: colors.border }}>
            <b>BYTE</b>
            <p>Here's how answers will look.</p>
            <p style={{ opacity: 0.6 }}>Faint labels and hints</p>
            <span className="tp-btn" style={{ background: colors.accent, color: /^#[0-9a-f]{6}$/i.test(colors.bg) && parseInt(colors.bg.slice(1), 16) > 0x999999 ? "#ffffff" : colors.bg }}>
              Send
            </span>
            <span className="tp-link" style={{ color: colors.accent }}>
              A link [1]
            </span>
          </div>
        </div>
      </div>
      {warnings.length > 0 && (
        <ul className="theme-warnings small">
          {warnings.map((w) => (
            <li key={w}>
              <AlertTriangle size={12} /> {w}
            </li>
          ))}
        </ul>
      )}
      <div className="row" style={{ gap: 8, flexWrap: "wrap", marginTop: 8 }}>
        <button className="btn sm primary" onClick={() => void saveTheme()}>
          Save and use
        </button>
        <button className="btn sm ghost" onClick={() => void exportIt()}>
          <Download size={13} /> Export
        </button>
        <button className="btn sm ghost" onClick={() => file.current?.click()}>
          <Upload size={13} /> Import…
        </button>
        <input ref={file} type="file" accept=".bytetheme,.json" hidden onChange={(e) => void importIt(e.target.files?.[0])} />
        {editing && (
          <button className="btn sm ghost" onClick={() => void remove()}>
            <Trash2 size={13} /> Delete
          </button>
        )}
      </div>
      {error && <div className="banner danger">{error}</div>}
    </div>
  );
}
