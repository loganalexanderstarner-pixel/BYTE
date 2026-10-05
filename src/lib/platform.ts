import { isWindows } from "./keys";
import type { SystemInfo } from "./types";

/**
 * True when the app is running on a PC rather than a Mac. The backend's own word wins
 * (`system.platform`); until it has answered, or from an older backend that does not send
 * it, the webview's platform decides.
 */
export function isPc(system?: Pick<SystemInfo, "platform"> | null): boolean {
  if (system?.platform) return system.platform !== "macos";
  return isWindows();
}

/** What to call the computer in running text: "Mac" or "PC". */
export function machine(system?: Pick<SystemInfo, "platform"> | null): "Mac" | "PC" {
  return isPc(system) ? "PC" : "Mac";
}

/** "AMD Ryzen 7 7700X 8-Core Processor" → "AMD Ryzen 7 7700X". */
export function cpuName(raw: string): string {
  return raw
    .replace(/\((R|TM)\)/gi, "")
    .replace(/\b\d+-Core Processor\b/i, "")
    .replace(/\bProcessor\b/i, "")
    .replace(/\s+@\s*[\d.]+\s*GHz/i, "")
    .replace(/\s{2,}/g, " ")
    .trim();
}

/** The graphics card that will run the model, as "NVIDIA GeForce RTX 4070 · 12 GB". */
export function graphicsLabel(system: Pick<SystemInfo, "gpus">): string {
  const gpus = system.gpus ?? [];
  const card = gpus.find((g) => !g.integrated) ?? gpus[0];
  if (!card) return "None found";
  if (card.integrated) return `${card.name} (integrated)`;
  const gb = Math.round(card.dedicatedBytes / 2 ** 30);
  return `${card.name} · ${gb} GB`;
}

/** The one-line verdict on first launch. Never says a PC is unsupported: any hardware runs BYTE. */
export function hardwareNote(system: SystemInfo): { ok: boolean; text: string } | null {
  const gb = system.totalRamBytes / 2 ** 30;
  if (!isPc(system)) {
    // The Mac wording, exactly as it was.
    if (!system.appleSilicon) return { ok: false, text: "BYTE is built for Apple Silicon Macs (M1 or newer). It may not run well here." };
    if (gb < 12) return { ok: false, text: "With 8 GB of memory, BYTE will use the smaller Fast model." };
    if (gb < 20) return { ok: true, text: "Your Mac can run BYTE's Smart model comfortably." };
    return { ok: true, text: "Your Mac has plenty of memory — every BYTE model will run well." };
  }
  const gpus = system.gpus ?? [];
  const card = gpus.find((g) => !g.integrated);
  if (card) {
    const vram = card.dedicatedBytes / 2 ** 30;
    const n = Math.round(vram);
    if (vram >= 12) return { ok: true, text: `Your ${card.name} has ${n} GB of video memory, enough to run BYTE's larger models quickly.` };
    if (vram >= 6) return { ok: true, text: `Your ${card.name} (${n} GB) runs BYTE's Fast and Smart models well. The largest ones will be shared between the card and your memory.` };
    return { ok: true, text: `Your ${card.name} (${n} GB) is small for AI, so BYTE will pick compact models and run some of the work on your processor.` };
  }
  if (gb < 12) return { ok: false, text: `No graphics card found and ${Math.round(gb)} GB of memory, so BYTE will use its smallest models on your processor. Answers will be slower.` };
  return { ok: true, text: "No dedicated graphics card found, so BYTE will run on your processor. The smaller models are the best fit, and answers will be slower than on a card." };
}

/** What the engine computes with, for "BYTE's built-in AI engine (…) runs on this Mac". A Mac, or an
 *  older backend that does not say, keeps the Mac wording. */
export function engineWith(system?: Pick<SystemInfo, "backend"> | null): string {
  switch (system?.backend) {
    case "cuda":
      return "llama.cpp with NVIDIA CUDA";
    case "vulkan":
      return "llama.cpp with Vulkan";
    case "cpu":
      return "llama.cpp on the processor";
    default:
      return "llama.cpp with Apple Metal";
  }
}

/**
 * Static UI copy, worded for the machine it is on. The identity on a Mac, so the Mac text is
 * exactly what it was; on Windows it swaps the Mac words for the PC ones: "this Mac" becomes
 * "this PC", Finder becomes File Explorer, the menu bar the system tray, the Keychain
 * Windows' Credential Manager, Touch ID Windows Hello.
 *
 * ONLY for strings the app itself wrote. Never run it over something a person or a model
 * wrote (a chat message, a note title): rewriting what someone typed is a bug. Rules apply in
 * order, because "your Mac's Keychain" has to become one phrase, not "your PC's Credential
 * Manager".
 */
export function osText(text: string, windows: boolean = isWindows()): string {
  if (!windows) return text;
  return text
    .replace(/\b(?:[Yy]our|[Tt]he|[Tt]his) (?:Mac|macOS)'s Keychain\b/g, "Windows Credential Manager")
    .replace(/\bthe macOS Keychain\b/g, "Windows Credential Manager")
    .replace(/\bKeychain\b/g, "Credential Manager")
    .replace(/\bApple Silicon GPU\b/g, "graphics card or processor")
    .replace(/\bShow in Finder\b/g, "Show in File Explorer")
    .replace(/\bFinder\b/g, "File Explorer")
    .replace(/\bmenu[- ]bar\b/gi, (m) => (m[0] === "M" ? "System tray" : "system tray"))
    .replace(/\bTouch ID\b/g, "Windows Hello")
    .replace(/\bMac control\b/g, "PC control")
    .replace(/\bSystem Settings → Accessibility → Display\b/g, "Settings → Accessibility → Visual effects")
    .replace(/\bSystem Settings\b/g, "Settings")
    .replace(/\bPrivacy & Security\b/g, "Privacy & security")
    .replace(/\bmacOS\b/g, "Windows")
    .replace(/\bMacs\b/g, "PCs")
    .replace(/\bMac(?='s\b)/g, "PC")
    .replace(/\bMac\b/g, "PC");
}
