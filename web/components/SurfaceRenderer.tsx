"use client";

import { Surface } from "@/lib/types";

export function WeatherSurface({ data }: { data: Record<string, unknown> }) {
  const temp = data.temp as string | number ?? "—";
  const condition = data.condition as string ?? "";
  const location = data.location as string ?? "";
  return (
    <div className="surface-fill">
      <div className="label">weather{location ? ` · ${location}` : ""}</div>
      <div className="surface-value serif">{temp}°</div>
      <div className="muted">{condition}</div>
    </div>
  );
}

export function SpotifySurface({ data }: { data: Record<string, unknown> }) {
  const track = data.track as string ?? "Unknown";
  const artist = data.artist as string ?? "";
  const progress = data.progress as number | undefined;
  const duration = data.duration as number | undefined;
  return (
    <div className="surface-fill surface-fill--spotify">
      <div className="label">now playing</div>
      <div className="surface-track">{track}</div>
      <div className="muted">{artist}</div>
      {typeof progress === "number" && typeof duration === "number" && duration > 0 && (
        <div className="surface-progress">
          <div className="surface-progress-bar" style={{ width: `${(progress / duration) * 100}%` }} />
        </div>
      )}
    </div>
  );
}

export function CalendarSurface({ data }: { data: Record<string, unknown> }) {
  const title = data.title as string ?? "No events";
  const time = data.time as string ?? "";
  const subtitle = data.subtitle as string ?? "";
  return (
    <div className="surface-fill">
      <div className="label">next up</div>
      <div className="surface-track">{title}</div>
      <div className="muted">{time}{subtitle ? ` · ${subtitle}` : ""}</div>
    </div>
  );
}

export function TasksSurface({ data }: { data: Record<string, unknown> }) {
  const items = (data.items as Array<{ text: string; done?: boolean }>) ?? [];
  const pending = items.filter((t) => !t.done).length;
  return (
    <div className="surface-fill">
      <div className="label">tasks</div>
      {items.length === 0 ? (
        <>
          <div className="surface-track">All clear</div>
          <div className="muted">Nothing pending</div>
        </>
      ) : (
        <>
          <div className="surface-track">{pending} pending</div>
          <ul className="surface-task-list">
            {items.slice(0, 5).map((item, i) => (
              <li key={i} className={item.done ? "done" : ""}>{item.text}</li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}

export function NewsSurface({ data }: { data: Record<string, unknown> }) {
  const headline = data.headline as string ?? "";
  const source = data.source as string ?? "";
  return (
    <div className="surface-fill">
      <div className="label">news{source ? ` · ${source}` : ""}</div>
      <div className="surface-track">{headline}</div>
    </div>
  );
}

export function QuoteSurface({ data }: { data: Record<string, unknown> }) {
  const text = data.text as string ?? "";
  const author = data.author as string ?? "";
  return (
    <div className="surface-fill surface-fill--quote">
      <div className="surface-quote serif">&ldquo;{text}&rdquo;</div>
      {author && <div className="muted">— {author}</div>}
    </div>
  );
}

export function CustomSurface({ surface }: { surface: Surface }) {
  const label = (surface.data.label as string) ?? "surface";
  const body = surface.html ?? "";
  return (
    <div className="surface-fill">
      <div className="label">{label}</div>
      <div className="surface-custom-body">{body}</div>
    </div>
  );
}

export function renderSurface(surface: Surface) {
  switch (surface.kind) {
    case "weather":
      return <WeatherSurface key={surface.id} data={surface.data} />;
    case "spotify":
      return <SpotifySurface key={surface.id} data={surface.data} />;
    case "calendar":
      return <CalendarSurface key={surface.id} data={surface.data} />;
    case "tasks":
      return <TasksSurface key={surface.id} data={surface.data} />;
    case "news":
      return <NewsSurface key={surface.id} data={surface.data} />;
    case "quote":
      return <QuoteSurface key={surface.id} data={surface.data} />;
    case "custom":
      return <CustomSurface key={surface.id} surface={surface} />;
    default:
      return null;
  }
}

// Default surfaces — always shown until the AI pushes real ones
export const DEFAULT_WEATHER: Surface = {
  id: "default-weather",
  kind: "weather",
  priority: 10,
  data: { temp: "—", condition: "Connecting...", location: "" },
  updated_at: "",
};

export const DEFAULT_QUOTE: Surface = {
  id: "default-quote",
  kind: "quote",
  priority: 5,
  data: {
    text: "The best way to predict the future is to invent it.",
    author: "Alan Kay",
  },
  updated_at: "",
};
