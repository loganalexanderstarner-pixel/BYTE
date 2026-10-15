import { Brain, Cloud, Cpu, HardDrive, LifeBuoy, Mic, Plug, Settings2, Shield } from "lucide-react";

import type { SettingsGroup } from "../../state/store";

/** Settings' groups, in order (the left list; also listed in the command palette). */
export const TABS: { id: SettingsGroup; label: string; icon: typeof Cpu }[] = [
  { id: "general", label: "General", icon: Settings2 },
  { id: "models", label: "Models and speed", icon: HardDrive },
  { id: "voice", label: "Voice and Hey BYTE", icon: Mic },
  { id: "control", label: "Control and screen", icon: Plug },
  { id: "privacy", label: "Privacy", icon: Shield },
  { id: "memory", label: "Memory and notes", icon: Brain },
  { id: "cloud", label: "Account and cloud", icon: Cloud },
  { id: "backup", label: "Backup and help", icon: LifeBuoy },
];
