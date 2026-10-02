import {
  Brain,
  Cloud,
  Cpu,
  FolderSearch,
  HardDrive,
  Info,
  Palette,
  Plug,
  Shield,
} from "lucide-react";

import type { SettingsTab } from "../../state/store";

/** Settings' sections, in order (also listed in the command palette). */
export const TABS: { id: SettingsTab; label: string; icon: typeof Cpu }[] = [
  { id: "models", label: "Models", icon: HardDrive },
  { id: "memory", label: "Memory & chats", icon: Brain },
  { id: "knowledge", label: "Knowledge base", icon: FolderSearch },
  { id: "connectors", label: "Connectors", icon: Plug },
  { id: "appearance", label: "Appearance", icon: Palette },
  { id: "engine", label: "Engine", icon: Cpu },
  { id: "cloud", label: "Cloud", icon: Cloud },
  { id: "privacy", label: "Privacy", icon: Shield },
  { id: "about", label: "About", icon: Info },
];
