import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

function App() {
  const [input, setInput] = useState("");
  const [output, setOutput] = useState("");
  const [isStreaming, setIsStreaming] = useState(false);

  useEffect(() => {
    let unlistenStream: (() => void) | undefined;
    let unlistenDone: (() => void) | undefined;

    (async () => {
      unlistenStream = await listen<string>("stream", (event) => {
        setOutput((prev) => prev + event.payload);
      });

      unlistenDone = await listen("stream_done", () => {
        setIsStreaming(false);
      });
    })();

    return () => {
      if (unlistenStream) unlistenStream();
      if (unlistenDone) unlistenDone();
    };
  }, []);

  const handleSend = async () => {
    if (!input.trim() || isStreaming) return;
    setOutput("");
    setIsStreaming(true);

    await invoke("stream_generate", {
      prompt: input.trim(),
    });
  };

  return (
    <div
      style={{
        height: "100vh",
        background: "radial-gradient(circle at top, #1f2937, #020617)",
        color: "#e5e7eb",
        fontFamily: "system-ui, -apple-system, BlinkMacSystemFont, sans-serif",
        display: "flex",
        flexDirection: "column",
        padding: "16px",
      }}
    >
      <h1 style={{ fontSize: "24px", marginBottom: "8px" }}>BYTE Command Center</h1>
      <p style={{ marginBottom: "16px", opacity: 0.8 }}>
        Local model: <strong>llama3:latest</strong> · Mode: <strong>smart</strong>
      </p>

      <div
        style={{
          flex: 1,
          borderRadius: "12px",
          border: "1px solid rgba(148, 163, 184, 0.4)",
          padding: "12px",
          background:
            "linear-gradient(135deg, rgba(15,23,42,0.9), rgba(30,64,175,0.25))",
          overflowY: "auto",
          whiteSpace: "pre-wrap",
          fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas",
          fontSize: "14px",
        }}
      >
        {output || "BYTE is idle. Type a message to begin."}
      </div>

      <div
        style={{
          marginTop: "12px",
          display: "flex",
          gap: "8px",
        }}
      >
        <input
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              handleSend();
            }
          }}
          placeholder="Ask BYTE something..."
          style={{
            flex: 1,
            borderRadius: "999px",
            border: "1px solid rgba(148, 163, 184, 0.6)",
            padding: "10px 14px",
            backgroundColor: "rgba(15,23,42,0.9)",
            color: "#e5e7eb",
            outline: "none",
          }}
        />
        <button
          onClick={handleSend}
          disabled={isStreaming || !input.trim()}
          style={{
            borderRadius: "999px",
            padding: "10px 18px",
            border: "none",
            background:
              "linear-gradient(135deg, #22c55e, #16a34a)",
            color: "#020617",
            fontWeight: 600,
            cursor: isStreaming || !input.trim() ? "not-allowed" : "pointer",
            opacity: isStreaming || !input.trim() ? 0.6 : 1,
          }}
        >
          {isStreaming ? "Streaming..." : "Send"}
        </button>
      </div>
    </div>
  );
}

export default App;
