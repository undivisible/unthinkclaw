import { describe, it, expect, vi, beforeEach } from "vitest";
import { gatewayHttp, gatewayWs, gatewayHeaders } from "../gateway";

describe("gatewayHttp", () => {
  it("constructs HTTP URL with path", () => {
    expect(gatewayHttp("/api/chat")).toBe("http://127.0.0.1:8080/api/chat");
  });

  it("handles empty path", () => {
    expect(gatewayHttp("")).toBe("http://127.0.0.1:8080");
  });
});

describe("gatewayWs", () => {
  it("constructs WebSocket URL with path", () => {
    expect(gatewayWs("/ws/session-1")).toBe("ws://127.0.0.1:8080/ws/session-1");
  });
});

describe("gatewayHeaders", () => {
  it("returns empty object when no token", () => {
    expect(gatewayHeaders()).toEqual({});
  });
});
