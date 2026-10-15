/** Spoken commands that change whose voice BYTE follows after "Hey BYTE" (voicefocus.rs keeps the state). */
export type FocusCommand = { kind: "everyone" } | { kind: "stopFocusing" } | { kind: "alsoListenTo"; name: string };

const norm = (text: string): string =>
  text
    .toLowerCase()
    .replace(/[’']/g, "'")
    .replace(/[^a-z' ]/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .replace(/^((hey|ok|okay) )?(byte )?(please )?/, "")
    .replace(/( please| byte| thanks| thank you)$/, "")
    .trim();

const title = (s: string): string => s.replace(/\b[a-z]/g, (c) => c.toUpperCase());

/** "listen to everyone" / "stop focusing on me" / "also listen to Anna" → the command, else null (an ordinary question). */
export function parseFocusCommand(text: string): FocusCommand | null {
  const t = norm(text);
  if (/^(listen to|hear) (everyone|everybody|all voices|anyone)( again| now)?$/.test(t)) return { kind: "everyone" };
  if (/^(stop focus(ing|ed)( on me)?|don't focus on me|stop (only )?listening (only )?to me|stop focusing on my voice)$/.test(t)) return { kind: "stopFocusing" };
  // A name is ONE short word; "also listen to this song" or "and hear the radio" are ordinary sentences. The caller still only acts on
  // it when the word is a taught voice, and sends the sentence on as normal conversation otherwise.
  const m = /^(also|and) (listen to|hear) ([a-z']{2,14})(?: too| as well)?$/.exec(t);
  if (m && !/^(everyone|everybody|me|anyone|this|that|it|them|him|her|music|the|a)$/.test(m[3])) return { kind: "alsoListenTo", name: title(m[3]) };
  return null;
}

/** Whether `name` is one of the taught voices (case does not matter). */
export function isTaught(name: string, taught: string[]): boolean {
  const n = name.trim().toLowerCase();
  return taught.some((t) => t.trim().toLowerCase() === n);
}
