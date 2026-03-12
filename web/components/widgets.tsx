"use client";

import { useEffect, useMemo, useState } from "react";
import { HubWidget } from "@/lib/types";
import { fetchGatewayJson } from "@/lib/gateway";

function pretty(value: unknown) {
  if (typeof value === "number") return value.toLocaleString();
  if (typeof value === "string") return value;
  if (Array.isArray(value)) return `${value.length} items`;
  if (value && typeof value === "object") return JSON.stringify(value, null, 2);
  return "n/a";
}

function useEndpointData(endpoint?: string) {
  const [data, setData] = useState<unknown>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(!!endpoint);

  useEffect(() => {
    let cancelled = false;
    if (!endpoint) return;

    setLoading(true);
    fetchGatewayJson<unknown>(endpoint)
      .then((result) => {
        if (!cancelled) {
          setData(result);
          setLoading(false);
        }
      })
      .catch((err: Error) => {
        if (!cancelled) {
          setError(err.message);
          setLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [endpoint]);

  return { data, error, loading };
}

function LoadingSkeleton() {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10, marginTop: 12 }}>
      <div className="widget-skeleton" style={{ height: 28, width: "40%" }} />
      <div className="widget-skeleton" style={{ height: 14, width: "70%" }} />
    </div>
  );
}

export function WidgetCard({ widget }: { widget: HubWidget }) {
  const { data, error, loading } = useEndpointData(widget.endpoint);
  const className = widget.full ? "widget-card full" : "widget-card";

  const content = useMemo(() => {
    if (loading) return <LoadingSkeleton />;
    if (error) return <div className="muted" style={{ marginTop: 10 }}>{error}</div>;
    if (!data) return <div className="muted" style={{ marginTop: 10 }}>No data available.</div>;

    if (widget.kind === "metric") {
      const metricValue =
        typeof data === "object" && data !== null
          ? Object.values(data as Record<string, unknown>).find((v) => typeof v === "number")
          : data;
      return (
        <>
          <div className="metric-value">{pretty(metricValue)}</div>
          {widget.description && (
            <div className="muted" style={{ marginTop: 8 }}>{widget.description}</div>
          )}
        </>
      );
    }

    if (Array.isArray(data)) {
      return (
        <div style={{ display: "flex", flexDirection: "column", gap: 8, marginTop: 12 }}>
          {data.slice(0, 8).map((item, index) => (
            <div key={index} className="event-item">
              <pre style={{ margin: 0, whiteSpace: "pre-wrap" }}>{pretty(item)}</pre>
            </div>
          ))}
        </div>
      );
    }

    return (
      <pre style={{ marginTop: 12, whiteSpace: "pre-wrap", fontSize: "0.82rem", color: "var(--ink-soft)", lineHeight: 1.4 }}>
        {pretty(data)}
      </pre>
    );
  }, [data, error, loading, widget.description, widget.kind]);

  return (
    <section className={className}>
      <div style={{ display: "flex", justifyContent: "space-between", gap: 12, alignItems: "flex-start" }}>
        <div>
          <div className="serif" style={{ fontSize: "1.35rem", lineHeight: 1.1, fontWeight: 600 }}>
            {widget.title}
          </div>
          {widget.description && !loading && widget.kind !== "metric" && (
            <div className="muted" style={{ marginTop: 4 }}>{widget.description}</div>
          )}
        </div>
        <span className="status-chip">{widget.kind}</span>
      </div>
      {content}
    </section>
  );
}
