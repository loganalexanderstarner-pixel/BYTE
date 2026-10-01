import { useEffect } from "react";

import { DEFAULT_THEME, resolveTheme } from "../design/themes";
import type { Settings } from "../lib/types";
import { useStore } from "../state/store";
import { Onboarding } from "../components/onboarding/Onboarding";
import { QuickAsk } from "./QuickAsk";
import { Shell } from "./Shell";

/** Applies the theme, density and font size to the document root (every window). */
export function useAppearance(settings: Settings | null) {
  useEffect(() => {
    const root = document.documentElement;
    const apply = () => {
      root.dataset.theme = resolveTheme(settings?.theme ?? DEFAULT_THEME);
      root.dataset.density = settings?.density ?? "comfortable";
      root.style.setProperty("--font-scale", String(settings?.fontScale ?? 1));
    };
    apply();
    const mq = window.matchMedia("(prefers-color-scheme: light)");
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [settings?.theme, settings?.density, settings?.fontScale]);
}

/** Which window this is: "quick" (Quick Ask, quick.rs) or the main one. `?window=quick` stands in outside the app. */
export function windowKind(): "quick" | "main" {
  const meta = (window as { __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } } }).__TAURI_INTERNALS__;
  const label = new URLSearchParams(window.location.search).get("window") ?? meta?.metadata?.currentWindow?.label;
  return label === "quick" ? "quick" : "main";
}

export function App() {
  return windowKind() === "quick" ? <QuickAsk /> : <MainApp />;
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
