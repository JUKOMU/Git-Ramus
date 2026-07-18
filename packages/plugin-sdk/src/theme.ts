import { themeDefinitionSchema, type ThemeDefinition } from "@git-ramus/contracts";

export function applyThemeToDocument(theme: ThemeDefinition, documentRoot?: HTMLElement): void {
  const root =
    documentRoot ?? (typeof document === "undefined" ? undefined : document.documentElement);
  if (!root) return;
  const parsed = themeDefinitionSchema.safeParse(theme);
  if (!parsed.success) return;
  for (const [group, values] of Object.entries(parsed.data)) {
    if (group === "themeId" || group === "name" || values === undefined) continue;
    if (group === "density") {
      root.style.setProperty("--gr-density", String(values));
      continue;
    }
    for (const [key, value] of Object.entries(values)) {
      root.style.setProperty(`--gr-${group}-${key}`, String(value));
    }
  }
}
