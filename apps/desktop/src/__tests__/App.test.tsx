import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { App } from "../App";
import type { HostApi } from "../lib/hostApi";

const hostApi: HostApi = {
  getAppInfo: async () => ({ name: "Git-Ramus", version: "0.1.0" }),
  listPlugins: async () => [],
  listJobs: async () => [],
  authorizePluginCall: async () => ({ allowed: true }),
  startEchoJob: async () => ({
    id: "a032bc9c-8759-45ac-856f-b76f9addb9d1",
    kind: "system.echo",
    title: "Echo hello",
    status: "queued",
    progress: 0,
    cancelRequested: false,
    createdAt: "2026-07-17T00:00:00Z",
    updatedAt: "2026-07-17T00:00:00Z",
    error: null
  }),
  cancelJob: async () => undefined
};

describe("App", () => {
  it("renders the trusted shell and host version", async () => {
    render(<App hostApi={hostApi} />);
    expect(screen.getByRole("heading", { name: "Git-Ramus" })).toBeInTheDocument();
    expect(await screen.findByText("Host 0.1.0")).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Primary" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Tasks" })).toBeInTheDocument();
  });
});
