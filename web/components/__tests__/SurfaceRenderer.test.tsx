import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import {
  WeatherSurface,
  SpotifySurface,
  CalendarSurface,
  TasksSurface,
  NewsSurface,
  QuoteSurface,
  CustomSurface,
  renderSurface,
  DEFAULT_WEATHER,
  DEFAULT_QUOTE,
} from "../SurfaceRenderer";
import type { Surface } from "@/lib/types";

describe("WeatherSurface", () => {
  it("renders temperature and condition", () => {
    render(<WeatherSurface data={{ temp: 72, condition: "Sunny", location: "SF" }} />);
    expect(screen.getByText("72°")).toBeInTheDocument();
    expect(screen.getByText("Sunny")).toBeInTheDocument();
    expect(screen.getByText("weather · SF")).toBeInTheDocument();
  });

  it("renders defaults when data is empty", () => {
    render(<WeatherSurface data={{}} />);
    expect(screen.getByText("—°")).toBeInTheDocument();
  });
});

describe("SpotifySurface", () => {
  it("renders track and artist", () => {
    render(<SpotifySurface data={{ track: "Song A", artist: "Artist B" }} />);
    expect(screen.getByText("Song A")).toBeInTheDocument();
    expect(screen.getByText("Artist B")).toBeInTheDocument();
    expect(screen.getByText("now playing")).toBeInTheDocument();
  });

  it("renders progress bar when progress and duration given", () => {
    const { container } = render(
      <SpotifySurface data={{ track: "X", artist: "Y", progress: 30, duration: 100 }} />
    );
    const bar = container.querySelector(".surface-progress-bar");
    expect(bar).toBeInTheDocument();
    expect(bar).toHaveStyle({ width: "30%" });
  });

  it("omits progress bar when duration is 0", () => {
    const { container } = render(
      <SpotifySurface data={{ track: "X", artist: "Y", progress: 0, duration: 0 }} />
    );
    expect(container.querySelector(".surface-progress-bar")).not.toBeInTheDocument();
  });
});

describe("CalendarSurface", () => {
  it("renders title and time", () => {
    render(<CalendarSurface data={{ title: "Meeting", time: "2pm", subtitle: "Room 1" }} />);
    expect(screen.getByText("Meeting")).toBeInTheDocument();
    expect(screen.getByText("2pm · Room 1")).toBeInTheDocument();
  });
});

describe("TasksSurface", () => {
  it("renders pending count and items", () => {
    const items = [
      { text: "Buy milk", done: false },
      { text: "Ship feature", done: true },
      { text: "Call dentist", done: false },
    ];
    render(<TasksSurface data={{ items }} />);
    expect(screen.getByText("2 pending")).toBeInTheDocument();
    expect(screen.getByText("Buy milk")).toBeInTheDocument();
    expect(screen.getByText("Ship feature")).toBeInTheDocument();
  });

  it("renders empty state", () => {
    render(<TasksSurface data={{ items: [] }} />);
    expect(screen.getByText("All clear")).toBeInTheDocument();
  });

  it("handles missing items gracefully", () => {
    render(<TasksSurface data={{}} />);
    expect(screen.getByText("All clear")).toBeInTheDocument();
  });
});

describe("NewsSurface", () => {
  it("renders headline and source", () => {
    render(<NewsSurface data={{ headline: "Big Story", source: "Reuters" }} />);
    expect(screen.getByText("Big Story")).toBeInTheDocument();
    expect(screen.getByText("news · Reuters")).toBeInTheDocument();
  });
});

describe("QuoteSurface", () => {
  it("renders quote text and author", () => {
    render(<QuoteSurface data={{ text: "Hello world", author: "Dev" }} />);
    expect(screen.getByText(/Hello world/)).toBeInTheDocument();
    expect(screen.getByText("— Dev")).toBeInTheDocument();
  });

  it("omits author when not provided", () => {
    render(<QuoteSurface data={{ text: "Test" }} />);
    expect(screen.queryByText(/—/)).not.toBeInTheDocument();
  });
});

describe("CustomSurface", () => {
  it("renders label and body text", () => {
    const surface: Surface = {
      id: "c1",
      kind: "custom",
      priority: 1,
      data: { label: "my widget" },
      html: "Some content here",
      updated_at: "",
    };
    render(<CustomSurface surface={surface} />);
    expect(screen.getByText("my widget")).toBeInTheDocument();
    expect(screen.getByText("Some content here")).toBeInTheDocument();
  });
});

describe("renderSurface", () => {
  it("renders weather surface by kind", () => {
    const { container } = render(<>{renderSurface(DEFAULT_WEATHER)}</>);
    expect(container.querySelector(".surface-fill")).toBeInTheDocument();
    expect(screen.getByText("—°")).toBeInTheDocument();
  });

  it("renders quote surface by kind", () => {
    render(<>{renderSurface(DEFAULT_QUOTE)}</>);
    expect(screen.getByText(/predict the future/)).toBeInTheDocument();
  });

  it("returns null for unknown kind", () => {
    const unknown: Surface = {
      id: "x",
      kind: "nonexistent" as Surface["kind"],
      priority: 0,
      data: {},
      updated_at: "",
    };
    const { container } = render(<>{renderSurface(unknown)}</>);
    expect(container.innerHTML).toBe("");
  });
});

describe("Default surfaces", () => {
  it("DEFAULT_WEATHER has expected shape", () => {
    expect(DEFAULT_WEATHER.kind).toBe("weather");
    expect(DEFAULT_WEATHER.priority).toBe(10);
    expect(DEFAULT_WEATHER.data.temp).toBe("—");
  });

  it("DEFAULT_QUOTE has expected shape", () => {
    expect(DEFAULT_QUOTE.kind).toBe("quote");
    expect(DEFAULT_QUOTE.priority).toBe(5);
    expect(DEFAULT_QUOTE.data.author).toBe("Alan Kay");
  });
});
