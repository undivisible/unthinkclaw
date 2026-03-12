import { HubPlan } from "@/lib/types";

const gatewayHttpBase =
  process.env.NEXT_PUBLIC_GATEWAY_URL || "http://127.0.0.1:8080";

const gatewayWsBase =
  process.env.NEXT_PUBLIC_GATEWAY_WS_URL || "ws://127.0.0.1:8080";

export function gatewayHttp(path: string) {
  return `${gatewayHttpBase}${path}`;
}

export function gatewayWs(path: string) {
  return `${gatewayWsBase}${path}`;
}

export function gatewayHeaders(): Record<string, string> {
  const token = process.env.NEXT_PUBLIC_GATEWAY_TOKEN || "";
  return token
    ? {
        Authorization: `Bearer ${token}`,
        "x-unthinkclaw-token": token
      }
    : {};
}

export async function fetchGatewayJson<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(gatewayHttp(path), {
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...gatewayHeaders(),
      ...(init?.headers || {})
    },
    cache: "no-store"
  });

  if (!response.ok) {
    throw new Error(`Gateway request failed (${response.status})`);
  }

  return response.json() as Promise<T>;
}

export function defaultHubPlan(): HubPlan {
  return {
    title: "Adaptive Claw Hub",
    summary:
      "A tri-pane operating surface that balances personal coordination, business telemetry, and live agent execution.",
    widgets: [
      {
        id: "session-pulse",
        kind: "metric",
        title: "Session Pulse",
        description: "Live session and runtime heartbeat.",
        endpoint: "/api/status"
      },
      {
        id: "swarm-status",
        kind: "metric",
        title: "Swarm Status",
        description: "Workers, pending tasks, and queue pressure.",
        endpoint: "/api/swarm/status"
      },
      {
        id: "recent-sessions",
        kind: "list",
        title: "Recent Sessions",
        description: "Cross-device and cross-context continuity.",
        endpoint: "/api/sessions",
        full: true
      },
      {
        id: "plugin-surface",
        kind: "api",
        title: "Capability Surface",
        description: "Live plugin inventory for business and personal workflows.",
        endpoint: "/api/plugins"
      }
    ]
  };
}
