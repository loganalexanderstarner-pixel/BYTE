/** How BYTE talks beyond the sliders (style.rs): the modes the person can pick. */
export type TalkMode = "" | "casual" | "direct" | "unhinged" | "auto";

export const TALK_MODES: { id: TalkMode; name: string; hint: string }[] = [
  { id: "", name: "Just the sliders", hint: "No extra style: BYTE talks the way the sliders below say" },
  { id: "casual", name: "Casual", hint: "A relaxed friend: plain words, a light touch of humor" },
  { id: "direct", name: "Direct", hint: "The answer first, no pleasantries, blunt when you ask what it thinks" },
  { id: "unhinged", name: "Unhinged 18+", hint: "Swears freely and jokes crude and dark. For adults only" },
  { id: "auto", name: "Auto", hint: "Starts casual and slowly learns how you talk. One joke or one swear changes nothing" },
];

/** What the person must agree to before Unhinged (or swearing in Auto) is switched on. */
export const ADULT_WARNING =
  "Unhinged is an 18+ feature, and it is for entertainment only.\n\nBYTE will swear, roast you, and joke about crude, dark and even violent things, " +
  "sometimes when you are being serious.\n\nDon't take its advice. Use it for weather, timers, lists and facts, not for health, money, legal or real-life decisions. " +
  "It says all of this out loud too, so be careful around other people.\n\nOnly turn it on if you are an adult and are fine with that. Turn it on?";

export const SWEAR_WARNING =
  "This is an 18+ feature.\n\nBYTE will swear back, mildly, when you swear in your own messages. Only turn it on if you are an adult and are fine with that. Turn it on?";

/** Does picking this mode need the adult warning first? */
export const needsAdultWarning = (mode: TalkMode, allowSwearing: boolean | undefined) => mode === "unhinged" && !allowSwearing;
