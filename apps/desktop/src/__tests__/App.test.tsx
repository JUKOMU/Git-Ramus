import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { App } from "../App";
import type { HostApi } from "../lib/hostApi";

const hostApi: HostApi = {
  getAppInfo: async () => ({ name: "Git-Ramus", version: "0.1.0" })
};

describe("App", () => {
  it("renders the trusted shell and host version", async () => {
    render(<App hostApi={hostApi} />);
    expect(screen.getByRole("heading", { name: "Git-Ramus" })).toBeInTheDocument();
    expect(await screen.findByText("Host 0.1.0")).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Primary" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Tasks" })).toBeInTheDocument();
  });
});
