import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

describe("Provider Center styles", () => {
  it("delegates visual values to host theme and density tokens", async () => {
    const style = await readFile(resolve(process.cwd(), "src/style.css"), "utf8");
    expect(style).not.toMatch(/#[0-9a-f]{3,8}\b/iu);
    expect(style).not.toMatch(/\b(?:rgb|rgba|hsl|hsla)\s*\(/iu);
    expect(style).toMatch(/--gr-colors-/u);
    expect(style).toMatch(/--gr-spacing-/u);
    expect(style).toMatch(/--gr-shape-/u);
    expect(style).toMatch(/--gr-typography-/u);
  });
});
