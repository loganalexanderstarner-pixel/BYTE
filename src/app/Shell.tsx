import { PanelLeft, Settings as SettingsIcon, SquarePen } from "lucide-react";
import { useEffect } from "react";

import { ChatView } from "../components/chat/ChatView";
import { Composer } from "../components/chat/Composer";
import { EngineBadge } from "../components/EngineBadge";
import { SettingsModal } from "../components/settings/SettingsModal";
import { Sidebar } from "../components/Sidebar";
import { useStore } from "../state/store";

export function Shell() {
  const sidebarOpen = useStore((s) => s.sidebarOpen);
  const toggleSidebar = useStore((s) => s.toggleSidebar);
  const newChat = useStore((s) => s.newChat);
  const openSettings = useStore((s) => s.openSettings);
  const settingsTab = useStore((s) => s.settingsTab);
  const stop = useStore((s) => s.stop);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const cmd = e.metaKey || e.ctrlKey;
      if (cmd && e.key.toLowerCase() === "n") {
        e.preventDefault();
        newChat();
      } else if (cmd && e.key === ",") {
        e.preventDefault();
        openSettings("models");
      } else if (cmd && e.key === "\\") {
        e.preventDefault();
        toggleSidebar();
      } else if (e.key === "Escape" && !useStore.getState().settingsTab) {
        void stop();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [newChat, openSettings, toggleSidebar, stop]);

  return (
    <div className={`app ${sidebarOpen ? "" : "sidebar-hidden"}`}>
      <Sidebar />
      <main className="main">
        <header className="titlebar" data-tauri-drag-region>
          <div className="row">
            {!sidebarOpen && (
              <>
                <button className="icon-btn" onClick={toggleSidebar} title="Show sidebar (⌘\)">
                  <PanelLeft size={18} />
                </button>
                <button className="icon-btn" onClick={newChat} title="New chat (⌘N)">
                  <SquarePen size={18} />
                </button>
              </>
            )}
          </div>
          <div className="row no-drag">
            <EngineBadge />
            <button className="icon-btn" onClick={() => openSettings("models")} title="Settings (⌘,)">
              <SettingsIcon size={18} />
            </button>
          </div>
        </header>
        <ChatView />
        <Composer />
      </main>
      {settingsTab && <SettingsModal />}
    </div>
  );
}
