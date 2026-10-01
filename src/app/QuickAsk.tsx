import { ExternalLink, SquarePen } from "lucide-react";
import { useEffect, useRef } from "react";

import { ChatView } from "../components/chat/ChatView";
import { Composer } from "../components/chat/Composer";
import { Logo } from "../design/Logo";
import { api, inTauri } from "../lib/api";
import { prettyKeys } from "../lib/keys";
import { currentConversation, useStore } from "../state/store";
import { useAppearance } from "./App";

const focusBox = () => setTimeout(() => document.querySelector<HTMLTextAreaElement>(".composer textarea")?.focus(), 30);

/** Quick Ask (⌥Space, or the menu-bar icon): a small window over any app. Its chats are ordinary saved chats. */
export function QuickAsk() {
  const ready = useStore((s) => s.ready);
  const init = useStore((s) => s.init);
  const newChat = useStore((s) => s.newChat);
  const stop = useStore((s) => s.stop);
  const settings = useStore((s) => s.settings);
  const conv = useStore(currentConversation);
  const busy = useStore((s) => s.running.length > 0);
  const wasBusy = useRef(false);
  useAppearance(settings);

  useEffect(() => {
    void init().then(() => {
      // Start fresh rather than on the main window's last chat.
      useStore.getState().newChat();
      focusBox();
    });
  }, [init]);

  // Shown again: pick up settings changed in the main window, and put the cursor in the box.
  useEffect(() => {
    if (!inTauri) return;
    const off = api.onQuickShown(() => {
      void api.settingsGet().then((s) => useStore.setState({ settings: s }));
      focusBox();
    });
    return () => void off.then((f) => f());
  }, []);

  // An answer finished: the main window adds the chat to its list.
  useEffect(() => {
    if (wasBusy.current && !busy && inTauri) {
      // After the save (store.ts saves 300 ms after a change).
      setTimeout(() => void import("@tauri-apps/api/event").then(({ emit }) => emit("quick://saved")), 700);
    }
    wasBusy.current = busy;
  }, [busy]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        if (useStore.getState().running.length) void stop();
        else if (inTauri) void api.quickHide();
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "n") {
        e.preventDefault();
        newChat();
        focusBox();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [newChat, stop]);

  if (!ready) return null;
  const has = !!conv && conv.messages.length > 0;
  return (
    <div className="quick">
      <header className="quick-head" data-tauri-drag-region>
        <span className="quick-title" data-tauri-drag-region>
          <Logo size={18} /> Quick Ask
        </span>
        <span className="spacer" data-tauri-drag-region />
        {has && (
          <button
            className="btn sm ghost"
            onClick={() => {
              newChat();
              focusBox();
            }}
            title="New question (⌘N)"
          >
            <SquarePen size={13} /> New
          </button>
        )}
        <button
          className="btn sm ghost"
          disabled={!has || busy}
          onClick={() => void api.quickOpen(conv?.private ? null : (conv?.id ?? null))}
          title={busy ? "After the answer finishes" : "Continue this chat in BYTE's main window"}
        >
          <ExternalLink size={13} /> Open in BYTE
        </button>
      </header>
      {has ? (
        <ChatView />
      ) : (
        <div className="quick-hint muted">
          <p>
            Ask anything. <kbd>Esc</kbd> hides this window; <kbd>{prettyKeys(settings?.quickAskKeys ?? "Alt+Space")}</kbd> brings it back.
          </p>
        </div>
      )}
      <Composer />
    </div>
  );
}
