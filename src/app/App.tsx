import { useEffect, useState } from "react";

import { DEFAULT_THEME, resolveTheme } from "../design/themes";
import { applyCustom, isCustom, parseTheme } from "../lib/customTheme";
import { syncTitlebar } from "../lib/titlebar";
import type { Settings } from "../lib/types";
import { useStore } from "../state/store";
import { Onboarding } from "../components/onboarding/Onboarding";
import { LockScreen, useLock } from "../components/LockScreen";
import { api, inTauri } from "../lib/api";
import { QuickAsk } from "./QuickAsk";
import { Shell } from "./Shell";

/** Applies the theme, density and font size to the document root (every window). */
export function useAppearance(settings: Settings | null) {
  useEffect(() => {
    const root = document.documentElement;
    const apply = () => {
      const id = settings?.theme ?? DEFAULT_THEME;
      const custom = isCustom(id) ? ((settings?.customThemes ?? []).map(parseTheme).find((t) => t?.id === id) ?? null) : null;
      root.dataset.theme = resolveTheme(custom || isCustom(id) ? DEFAULT_THEME : id);
      applyCustom(root, custom);
      syncTitlebar(root);
      if (settings?.reduceMotion === "reduce") root.dataset.motion = "reduce";
      else delete root.dataset.motion;
      root.dataset.density = settings?.density ?? "comfortable";
      root.style.setProperty("--font-scale", String(settings?.fontScale ?? 1));
    };
    apply();
    const mq = window.matchMedia("(prefers-color-scheme: light)");
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [settings?.theme, settings?.density, settings?.fontScale, settings?.customThemes, settings?.reduceMotion]);
}

/** Which window this is: "quick" (Quick Ask, quick.rs) or the main one. `?window=quick` stands in outside the app. */
export function windowKind(): "quick" | "main" {
  const meta = (window as { __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } } }).__TAURI_INTERNALS__;
  const label = new URLSearchParams(window.location.search).get("window") ?? meta?.metadata?.currentWindow?.label;
  return label === "quick" ? "quick" : "main";
}

export function App() {
  const locked = useLock();
  const quick = windowKind() === "quick";
  if (locked === null) return null;
  // Locked: nothing that reads chats is mounted until unlocking.
  if (locked) return <Locked compact={quick} />;
  return quick ? <QuickAsk /> : <MainApp />;
}

function Locked({ compact }: { compact: boolean }) {
  const [settings, setSettings] = useState<Settings | null>(null);
  useEffect(() => {
    if (inTauri) void api.settingsGet().then(setSettings, () => undefined);
  }, []);
  useAppearance(settings);
  return <LockScreen compact={compact} />;
}

function MainApp() {
  const ready = useStore((s) => s.ready);
  const settings = useStore((s) => s.settings);
  const init = useStore((s) => s.init);

  useEffect(() => {
    void init();
  }, [init]);
  useAppearance(settings);

  if (!ready) return null;
  if (settings && !settings.onboardingComplete) return <Onboarding />;
  return <Shell />;
}
