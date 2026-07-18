import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { HostApi } from "../lib/hostApi";
import { App } from "../App";

const plugin = {
  manifest: {
    schemaVersion: 1 as const,
    id: "git-ramus.welcome",
    name: "Welcome",
    version: "0.1.0",
    publisher: "git-ramus",
    description: "Welcome plugin",
    kind: "builtin" as const,
    sdkVersion: "^0.1.0",
    entrypoints: { ui: "ui.html" },
    contributions: {
      navigation: [{ id: "welcome", label: "Welcome", route: "/welcome", icon: "sparkles" }]
    },
    permissions: [
      { capability: "app:read", resources: ["info"] },
      { capability: "tasks:create", resources: ["echo"] }
    ]
  },
  uiUrl: "http://git-ramus-plugin.localhost/git-ramus.welcome/ui.html"
};

describe("foundation flow", () => {
  it("adds bundled navigation, opens the sandbox, and shows persistent tasks", async () => {
    const user = userEvent.setup();
    const hostApi: HostApi = {
      getAppInfo: vi.fn(async () => ({ name: "Git-Ramus", version: "0.1.0" })),
      listPlugins: vi.fn(async () => [plugin]),
      listJobs: vi.fn(async () => [
        {
          id: "a032bc9c-8759-45ac-856f-b76f9addb9d1",
          kind: "system.echo",
          title: "Echo hello",
          status: "running" as const,
          progress: 0.5,
          cancelRequested: false,
          createdAt: "2026-07-17T00:00:00Z",
          updatedAt: "2026-07-17T00:00:01Z",
          error: null
        }
      ]),
      authorizePluginCall: vi.fn(async () => ({ allowed: true })),
      startEchoJob: vi.fn(),
      cancelJob: vi.fn(async () => undefined)
    };
    render(<App hostApi={hostApi} />);
    await user.click(await screen.findByRole("button", { name: "Welcome" }));
    expect(screen.getByTitle("Welcome plugin")).toHaveAttribute("sandbox", "allow-scripts");
    expect(await screen.findByText("Echo hello")).toBeInTheDocument();
    expect(screen.getByText("50%")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(hostApi.cancelJob).toHaveBeenCalledWith("a032bc9c-8759-45ac-856f-b76f9addb9d1");
  });
});
