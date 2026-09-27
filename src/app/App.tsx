import { useEffect } from "react";

import { resolveTheme } from "../design/themes";
import { useStore } from "../state/store";
import { Onboarding } from "../components/onboarding/Onboarding";
import { Shell } from "./Shell";

export function App() {
  const ready = useStore((s) => s.ready);
  const settings = useStore((s) => s.settings);
  const init = useStore((s) => s.init);

  useEffect(() => {
    void init();
  }, [init]);

  // Apply appearance settings to the document root.
  useEffect(() => {
    const root = document.documentElement;
    const apply = () => {
      root.dataset.theme = resolveTheme(settings?.theme ?? "neon-night");
      root.dataset.density = settings?.density ?? "comfortable";
      root.style.setProperty("--font-scale", String(settings?.fontScale ?? 1));
    };
    apply();
    const mq = window.matchMedia("(prefers-color-scheme: light)");
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [settings?.theme, settings?.density, settings?.fontScale]);

  if (!ready) return null;
  if (settings && !settings.onboardingComplete) return <Onboarding />;
  return <Shell />;
}
