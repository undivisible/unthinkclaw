"use client";

import Link from "next/link";
import { FormEvent, useEffect, useRef, useState } from "react";
import { gatewayHeaders, gatewayHttp } from "@/lib/gateway";
import { ChatEnvelope } from "@/lib/types";

type ChatItem = {
  role: "user" | "assistant";
  text: string;
};

const pills = [
  { label: "who", question: "who are you and what can you do?" },
  { label: "what", question: "what makes you different from other AI assistants?" },
  { label: "when", question: "when should I use you instead of ChatGPT?" },
  { label: "where", question: "where does my data live?" },
  { label: "why", question: "why should I trust a single binary over a cloud platform?" },
  { label: "how", question: "how do I get started?" },
];

export function LandingPage() {
  const [messages, setMessages] = useState<ChatItem[]>([
    { role: "user", text: "what makes you different?" },
    {
      role: "assistant",
      text: "I'm a single Rust binary under 10MB that runs your entire AI operating system \u2014 Telegram, Discord, Slack, WhatsApp, and seven more channels, all wired to whichever model you choose. I keep memory in SurrealDB with full-text search, spawn parallel sub-agents when the job is big, and start in under 10 milliseconds. No Docker, no microservices, no cloud lock-in. Just one binary, one config file, and you own everything.",
    },
  ]);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const chatRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (chatRef.current) {
      chatRef.current.scrollTop = chatRef.current.scrollHeight;
    }
  }, [messages]);

  async function send(text: string) {
    if (!text.trim() || sending) return;
    const trimmed = text.trim();

    setMessages((cur) => [...cur, { role: "user", text: trimmed }]);
    setDraft("");
    setSending(true);

    try {
      const payload: ChatEnvelope = {
        text: trimmed,
        user_id: "landing-visitor",
        channel: "web",
        agent_id: "main",
      };

      const response = await fetch(gatewayHttp("/api/chat"), {
        method: "POST",
        headers: { "Content-Type": "application/json", ...gatewayHeaders() },
        body: JSON.stringify(payload),
      });

      const result = await response.json();
      setMessages((cur) => [
        ...cur,
        { role: "assistant", text: result.text || "No response." },
      ]);
    } catch {
      setMessages((cur) => [
        ...cur,
        {
          role: "assistant",
          text: "Gateway isn't running right now. Get yours to chat live.",
        },
      ]);
    } finally {
      setSending(false);
    }
  }

  function handleSubmit(e: FormEvent) {
    e.preventDefault();
    send(draft);
  }

  return (
    <main className="shell landing-grid">
      <section className="landing-stage">
        <div ref={chatRef} className="landing-conversation">
          {messages.map((msg, i) => (
            <div
              key={i}
              className={`landing-msg ${msg.role === "user" ? "q" : "a"}`}
            >
              <div className="msg-role">{msg.role === "user" ? "you" : "claw"}</div>
              {msg.text}
            </div>
          ))}
          {sending && (
            <div className="landing-msg a">
              <div className="msg-role">claw</div>
              <span className="thinking-dots">thinking</span>
            </div>
          )}
        </div>
      </section>

      <section className="landing-bottom">
        <div className="question-row serif">
          {pills.map((p) => (
            <button
              key={p.label}
              type="button"
              className="question-pill"
              onClick={() => send(p.question)}
              disabled={sending}
            >
              {p.label}
            </button>
          ))}
        </div>

        <form onSubmit={handleSubmit}>
          <input
            type="text"
            className="hero-input serif"
            placeholder="ask claw anything..."
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            aria-label="Ask claw a question"
            disabled={sending}
          />
        </form>

        <div className="landing-footer">
          <div>
            <div className="wordmark serif">unthinkclaw</div>
            <div className="tagline">
              a faster, lighter, safer, richer agent runtime.
              <br />
              your personal AI operating system.
            </div>
          </div>

          <div>
            <Link href="/hub" className="cta serif">
              get yours
            </Link>
          </div>
        </div>
      </section>
    </main>
  );
}
