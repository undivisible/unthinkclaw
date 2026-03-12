export type HubWidgetKind = "metric" | "feed" | "list" | "api";

export interface HubWidget {
  id: string;
  kind: HubWidgetKind;
  title: string;
  description?: string;
  endpoint?: string;
  query?: string;
  full?: boolean;
}

export interface HubPlan {
  title: string;
  summary: string;
  widgets: HubWidget[];
}

export interface ChatEnvelope {
  text: string;
  user_id?: string;
  session_id?: string;
  channel?: string;
  agent_id?: string;
}

export interface SessionEvent {
  session_id: string;
  tenant_id: string;
  event: string;
  detail: Record<string, unknown>;
  emitted_at: string;
}

// ── AI-driven surfaces ──

export type SurfaceKind =
  | "weather"
  | "spotify"
  | "calendar"
  | "tasks"
  | "news"
  | "quote"
  | "custom";

export interface Surface {
  id: string;
  kind: SurfaceKind;
  priority: number;         // higher = more important right now
  data: Record<string, unknown>;
  /** For kind="custom": raw HTML the AI generated on-the-fly */
  html?: string;
  updated_at: string;
}
