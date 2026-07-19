import { describe, expect, it, vi } from "vitest";
import { completeLauncherCleanup } from "./launcher-cleanup";

describe("Basic Tauri launcher cleanup", () => {
  it("attempts owned-profile cleanup after a launcher failure and aggregates both errors", async () => {
    const stopError = new Error("injected launcher stop failure");
    const cleanupError = new Error("injected profile cleanup failure");
    const stop = vi.fn().mockRejectedValue(stopError);
    const cleanup = vi.fn().mockRejectedValue(cleanupError);

    const rejection = completeLauncherCleanup(stop, cleanup).catch((error: unknown) => error);

    await expect(rejection).resolves.toBeInstanceOf(AggregateError);
    await expect(rejection).resolves.toMatchObject({ errors: [stopError, cleanupError] });
    expect(stop).toHaveBeenCalledOnce();
    expect(cleanup).toHaveBeenCalledOnce();
  });
});
