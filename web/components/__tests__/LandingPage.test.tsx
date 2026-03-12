import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { LandingPage } from "../LandingPage";

// Mock next/link
vi.mock("next/link", () => ({
  default: ({ children, href, ...props }: { children: React.ReactNode; href: string; [key: string]: unknown }) => (
    <a href={href} {...props}>{children}</a>
  ),
}));

// Mock gateway
vi.mock("@/lib/gateway", () => ({
  gatewayHttp: (path: string) => `http://test${path}`,
  gatewayHeaders: () => ({}),
}));

describe("LandingPage", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("renders pre-seeded conversation", () => {
    render(<LandingPage />);
    expect(screen.getByText("what makes you different?")).toBeInTheDocument();
    expect(screen.getByText(/single Rust binary/)).toBeInTheDocument();
  });

  it("renders question pills", () => {
    render(<LandingPage />);
    expect(screen.getByText("who")).toBeInTheDocument();
    expect(screen.getByText("what")).toBeInTheDocument();
    expect(screen.getByText("how")).toBeInTheDocument();
  });

  it("renders wordmark and CTA", () => {
    render(<LandingPage />);
    expect(screen.getByText("unthinkclaw")).toBeInTheDocument();
    expect(screen.getByText("get yours")).toBeInTheDocument();
  });

  it("does not render surface widgets", () => {
    const { container } = render(<LandingPage />);
    expect(container.querySelector(".landing-surfaces")).not.toBeInTheDocument();
  });

  it("shows thinking indicator while sending", async () => {
    let resolveResponse: (value: unknown) => void;
    global.fetch = vi.fn().mockReturnValue(
      new Promise((resolve) => { resolveResponse = resolve; })
    );

    render(<LandingPage />);
    const input = screen.getByLabelText("Ask claw a question");
    fireEvent.change(input, { target: { value: "test" } });
    fireEvent.submit(input.closest("form")!);

    expect(screen.getByText("thinking")).toBeInTheDocument();

    resolveResponse!({
      json: () => Promise.resolve({ text: "done" }),
    });

    await waitFor(() => {
      expect(screen.queryByText("thinking")).not.toBeInTheDocument();
    });
  });

  it("handles gateway error gracefully", async () => {
    global.fetch = vi.fn().mockRejectedValue(new Error("network"));

    render(<LandingPage />);
    const input = screen.getByLabelText("Ask claw a question");
    fireEvent.change(input, { target: { value: "hello" } });
    fireEvent.submit(input.closest("form")!);

    await waitFor(() => {
      expect(screen.getByText(/Gateway isn't running/)).toBeInTheDocument();
    });
  });

  it("disables input and pills while sending", async () => {
    let resolveResponse: (value: unknown) => void;
    global.fetch = vi.fn().mockReturnValue(
      new Promise((resolve) => { resolveResponse = resolve; })
    );

    render(<LandingPage />);
    const input = screen.getByLabelText("Ask claw a question");
    fireEvent.change(input, { target: { value: "test" } });
    fireEvent.submit(input.closest("form")!);

    expect(input).toBeDisabled();
    expect(screen.getByText("who")).toBeDisabled();

    resolveResponse!({
      json: () => Promise.resolve({ text: "done" }),
    });

    await waitFor(() => {
      expect(input).not.toBeDisabled();
    });
  });
});
