import { beforeEach, describe, expect, it, vi } from "vitest";
import { mountWelcome } from "../main";

describe("welcome plugin", () => {
  beforeEach(() => {
    document.body.innerHTML = '<main id="app"></main>';
  });

  it("loads host metadata through the SDK", async () => {
    const request = vi.fn(async () => ({ name: "Git-Ramus", version: "0.1.0" }));
    mountWelcome(document, { ready: Promise.resolve(), request });
    await vi.waitFor(() => {
      expect(request).toHaveBeenCalledWith("app.getInfo", {});
      expect(document.body.textContent).toContain("Connected to Git-Ramus 0.1.0");
    });
  });
});
