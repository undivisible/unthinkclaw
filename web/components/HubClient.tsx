"use client";

import { FormEvent, useEffect, useRef, useState } from "react";
import { gatewayHeaders, gatewayHttp, gatewayWs } from "@/lib/gateway";
import { ChatEnvelope, Surface } from "@/lib/types";
import { renderSurface, DEFAULT_WEATHER, DEFAULT_QUOTE } from "./SurfaceRenderer";

type ChatItem = {
  role: "user" | "assistant";
  text: string;
};

export function HubClient() {
  const [authed, setAuthed] = useState(false);
  const [token, setToken] = useState("");
  const [messages, setMessages] = useState<ChatItem[]>([]);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [surfaces, setSurfaces] = useState<Surface[]>([]);
  const [sessionId] = useState(() => `web-${crypto.randomUUID()}`);
  const chatRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const saved = typeof window !== "undefined" && localStorage.getItem("claw-token");
    if (saved) {
      setToken(saved);
      setAuthed(true);
    }
  }, []);

  useEffect(() => {
    if (!authed) return;

    const socket = new WebSocket(gatewayWs(`/ws/${sessionId}`), []);

    socket.addEventListener("message", (ev) => {
      try {
        const payload = JSON.parse(ev.data) as {
          type?: string;
          surfaces?: Surface[];
          surface?: Surface;
        };

        if (payload.type === "surfaces" && payload.surfaces) {
          setSurfaces(
            [...payload.surfaces].sort((a, b) => b.priority - a.priority)
          );
        }

        if (payload.type === "surface" && payload.surface) {
          setSurfaces((cur) => {
            const filtered = cur.filter((s) => s.id !== payload.surface!.id);
            return [...filtered, payload.surface!].sort((a, b) => b.priority - a.priority);
          });
        }

        if (payload.type === "surface_remove" && payload.surface) {
          setSurfaces((cur) => cur.filter((s) => s.id !== payload.surface!.id));
        }
      } catch {
        /* keep socket tolerant */
      }
    });

    return () => socket.close();
  }, [authed, sessionId]);

  useEffect(() => {
    if (chatRef.current) {
      chatRef.current.scrollTop = chatRef.current.scrollHeight;
    }
  }, [messages]);

  function handleAuth(e: FormEvent) {
    e.preventDefault();
    if (token.trim()) {
      localStorage.setItem("claw-token", token.trim());
      setAuthed(true);
    }
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    const text = draft.trim();
    if (!text || sending) return;

    setMessages((cur) => [...cur, { role: "user", text }]);
    setDraft("");
    setSending(true);

    try {
      const payload: ChatEnvelope = {
        text,
        user_id: "web-user",
        session_id: sessionId,
        channel: "web",
        agent_id: "main",
      };

      const response = await fetch(gatewayHttp("/api/chat"), {
        method: "POST",
        headers: { "Content-Type": "application/json", ...gatewayHeaders() },
        body: JSON.stringify(payload),
      });

      const result = await response.json();
      setMessages((cur) => [...cur, { role: "assistant", text: result.text || "No response." }]);
    } catch {
      setMessages((cur) => [
        ...cur,
        { role: "assistant", text: "Couldn't reach the gateway. Make sure it's running." },
      ]);
    } finally {
      setSending(false);
    }
  }

  if (!authed) {
    return (
      <main className="hub-auth">
        <form onSubmit={handleAuth} className="hub-auth-form">
          <div className="serif hub-auth-title">unthinkclaw</div>
          <div className="muted">Enter your gateway token to continue.</div>
          <input
            type="password"
            value={token}
            onChange={(e) => setToken(e.target.value)}
            placeholder="Gateway token"
            className="hub-auth-input"
            autoFocus
          />
          <button type="submit" className="hub-auth-button">Enter</button>
        </form>
      </main>
    );
  }

  const topSurface = surfaces[0] ?? DEFAULT_WEATHER;
  const bottomSurface = surfaces[1] ?? DEFAULT_QUOTE;

  return (
    <main className="hub-shell">
      <section className="hub-pane hub-chat-pane">
        <div ref={chatRef} className="hub-chat-stream">
          {messages.length === 0 && (
            <div className="hub-empty">
              <div className="serif hub-empty-title">unthinkclaw</div>
              <div className="muted">
                Ask anything. Tell me how you want me to work, what to keep track of,
                or what to build for you.
              </div>
            </div>
          )}
          {messages.map((message, index) => (
            <div
              key={`${message.role}-${index}`}
              className={`msg ${message.role === "user" ? "user" : ""}`}
            >
              <span className="label">{message.role}</span>
              <div style={{ marginTop: 4 }}>{message.text}</div>
            </div>
          ))}
          {sending && (
            <div className="msg">
              <span className="label">claw</span>
              <div className="thinking-dots" style={{ marginTop: 4 }}>thinking</div>
            </div>
          )}
        </div>
        <form onSubmit={submit} className="composer">
          <input
            type="text"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                submit(e as unknown as FormEvent);
              }
            }}
            placeholder="Ask claw anything..."
            disabled={sending}
            aria-label="Message input"
          />
          <button type="submit" disabled={sending} aria-label="Send message">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
              <line x1="22" y1="2" x2="11" y2="13" />
              <polygon points="22 2 15 22 11 13 2 9 22 2" />
            </svg>
          </button>
        </form>
      </section>

      {/* Hidden on mobile via CSS */}
      <div className="hub-right-stack">
        <section className="hub-pane hub-surface-pane">
          {renderSurface(topSurface)}
        </section>
        <section className="hub-pane hub-surface-pane">
          {renderSurface(bottomSurface)}
        </section>
      </div>
    </main>
  );
}
