import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { HubClient } from "../HubClient";

// Mock gateway
vi.mock("@/lib/gateway", () => ({
  gatewayHttp: (path: string) => `http://test${path}`,
  gatewayWs: (path: string) => `ws://test${path}`,
  gatewayHeaders: () => ({}),
}));

// Mock WebSocket
class MockWebSocket {
  static instances: MockWebSocket[] = [];
  listeners: Record<string, Function[]> = {};
  readyState = 1;

  constructor() {
    MockWebSocket.instances.push(this);
  }
  addEventListener(event: string, cb: Function) {
    this.listeners[event] = this.listeners[event] || [];
    this.listeners[event].push(cb);
  }
  close() { this.readyState = 3; }
  emit(event: string, data: unknown) {
    (this.listeners[event] || []).forEach((cb) => cb(data));
  }
}

// Mock localStorage
const store: Record<string, string> = {};
const mockLocalStorage = {
  getItem: vi.fn((key: string) => store[key] ?? null),
  setItem: vi.fn((key: string, value: string) => { store[key] = value; }),
  removeItem: vi.fn((key: string) => { delete store[key]; }),
  clear: vi.fn(() => { for (const k in store) delete store[k]; }),
  get length() { return Object.keys(store).length; },
  key: vi.fn((i: number) => Object.keys(store)[i] ?? null),
};

Object.defineProperty(globalThis, "localStorage", { value: mockLocalStorage, writable: true });

beforeEach(() => {
  vi.restoreAllMocks();
  MockWebSocket.instances = [];
  (global as unknown as Record<string, unknown>).WebSocket = MockWebSocket as unknown as typeof WebSocket;
  for (const k in store) delete store[k];
  mockLocalStorage.getItem.mockImplementation((key: string) => store[key] ?? null);
  mockLocalStorage.setItem.mockImplementation((key: string, value: string) => { store[key] = value; });
});

describe("HubClient — auth gate", () => {
  it("shows auth form when not authenticated", () => {
    render(<HubClient />);
    expect(screen.getByPlaceholderText("Gateway token")).toBeInTheDocument();
    expect(screen.getByText("Enter")).toBeInTheDocument();
  });

  it("authenticates and shows hub on token submit", () => {
    render(<HubClient />);
    const input = screen.getByPlaceholderText("Gateway token");
    fireEvent.change(input, { target: { value: "my-token" } });
    fireEvent.submit(input.closest("form")!);

    expect(screen.getByText(/Ask anything/)).toBeInTheDocument();
    expect(store["claw-token"]).toBe("my-token");
  });

  it("auto-authenticates from localStorage", () => {
    store["claw-token"] = "saved-token";
    render(<HubClient />);
    expect(screen.getByText(/Ask anything/)).toBeInTheDocument();
  });

  it("does not authenticate with empty token", () => {
    render(<HubClient />);
    const input = screen.getByPlaceholderText("Gateway token");
    fireEvent.change(input, { target: { value: "   " } });
    fireEvent.submit(input.closest("form")!);

    expect(screen.getByPlaceholderText("Gateway token")).toBeInTheDocument();
  });
});

describe("HubClient — chat", () => {
  beforeEach(() => {
    store["claw-token"] = "test";
  });

  it("renders empty state when no messages", () => {
    render(<HubClient />);
    expect(screen.getByText(/Ask anything/)).toBeInTheDocument();
  });

  it("sends a message and shows response", async () => {
    global.fetch = vi.fn().mockResolvedValue({
      json: () => Promise.resolve({ text: "Hi there!" }),
    });

    render(<HubClient />);
    const input = screen.getByLabelText("Message input");
    fireEvent.change(input, { target: { value: "hello" } });
    fireEvent.submit(input.closest("form")!);

    expect(screen.getByText("hello")).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByText("Hi there!")).toBeInTheDocument();
    });
  });

  it("shows error message on fetch failure", async () => {
    global.fetch = vi.fn().mockRejectedValue(new Error("fail"));
    store["claw-token"] = "test";

    render(<HubClient />);
    const input = screen.getByLabelText("Message input");
    fireEvent.change(input, { target: { value: "test" } });
    fireEvent.submit(input.closest("form")!);

    await waitFor(() => {
      expect(screen.getByText(/Couldn't reach the gateway/)).toBeInTheDocument();
    });
  });

  it("does not send empty messages", () => {
    global.fetch = vi.fn();
    render(<HubClient />);
    const input = screen.getByLabelText("Message input");
    fireEvent.submit(input.closest("form")!);

    expect(global.fetch).not.toHaveBeenCalled();
  });
});

describe("HubClient — surfaces", () => {
  beforeEach(() => {
    store["claw-token"] = "test";
  });

  it("renders default surfaces", () => {
    render(<HubClient />);
    expect(screen.getByText("—°")).toBeInTheDocument();
    expect(screen.getByText(/predict the future/)).toBeInTheDocument();
  });

  it("updates surfaces via WebSocket", async () => {
    render(<HubClient />);

    await waitFor(() => expect(MockWebSocket.instances.length).toBeGreaterThan(0));

    const ws = MockWebSocket.instances[0];
    ws.emit("message", {
      data: JSON.stringify({
        type: "surfaces",
        surfaces: [
          { id: "s1", kind: "weather", priority: 10, data: { temp: 85, condition: "Hot", location: "LA" }, updated_at: "" },
          { id: "s2", kind: "news", priority: 5, data: { headline: "Breaking news", source: "AP" }, updated_at: "" },
        ],
      }),
    });

    await waitFor(() => {
      expect(screen.getByText("85°")).toBeInTheDocument();
      expect(screen.getByText("Breaking news")).toBeInTheDocument();
    });
  });

  it("handles single surface update", async () => {
    render(<HubClient />);

    await waitFor(() => expect(MockWebSocket.instances.length).toBeGreaterThan(0));

    const ws = MockWebSocket.instances[0];
    ws.emit("message", {
      data: JSON.stringify({
        type: "surface",
        surface: { id: "default-weather", kind: "weather", priority: 10, data: { temp: 60, condition: "Cloudy" }, updated_at: "" },
      }),
    });

    await waitFor(() => {
      expect(screen.getByText("60°")).toBeInTheDocument();
    });
  });
});
