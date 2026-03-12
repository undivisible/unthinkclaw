import { NextResponse } from "next/server";
import { defaultHubPlan, fetchGatewayJson, gatewayHeaders, gatewayHttp } from "@/lib/gateway";
import type { HubPlan } from "@/lib/types";

export async function GET() {
  const fallback = defaultHubPlan();

  try {
    const prompt = [
      "Return only JSON.",
      "Design a tri-pane hub layout for a user-facing AI operating system that serves both business and personal workflows.",
      "Prefer 4-6 widgets.",
      "Each widget must use one of these kinds: metric, feed, list, api.",
      "Include realistic endpoint paths that a hosted claw gateway could serve.",
      "JSON shape: { title: string, summary: string, widgets: Array<{ id, kind, title, description, endpoint?, query?, full? }> }"
    ].join(" ");

    const result = await fetch(gatewayHttp("/api/chat"), {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        ...gatewayHeaders()
      },
      body: JSON.stringify({
        text: prompt,
        user_id: "web-planner",
        session_id: "web-planner",
        channel: "web"
      }),
      cache: "no-store"
    });

    if (!result.ok) {
      return NextResponse.json(fallback);
    }

    const payload = (await result.json()) as { text?: string };
    const parsed = safeParsePlan(payload.text || "");
    return NextResponse.json(parsed ?? fallback);
  } catch {
    return NextResponse.json(fallback);
  }
}

function safeParsePlan(text: string): HubPlan | null {
  try {
    const raw = JSON.parse(text) as HubPlan;
    if (!raw || !Array.isArray(raw.widgets)) return null;
    return raw;
  } catch {
    const match = text.match(/\{[\s\S]*\}/);
    if (!match) return null;
    try {
      return JSON.parse(match[0]) as HubPlan;
    } catch {
      return null;
    }
  }
}
