import { themeDefinitionSchema, type ThemeDefinition } from "@git-ramus/contracts";

const appliedVariables = new WeakMap<HTMLElement, Set<string>>();

export function clearAppliedTheme(documentRoot?: HTMLElement): void {
  const root =
    documentRoot ?? (typeof document === "undefined" ? undefined : document.documentElement);
  if (!root) return;
  const previous = appliedVariables.get(root);
  if (!previous) return;
  for (const property of previous) root.style.removeProperty(property);
  appliedVariables.delete(root);
}

export function applyThemeToDocument(theme: ThemeDefinition, documentRoot?: HTMLElement): void {
  const root =
    documentRoot ?? (typeof document === "undefined" ? undefined : document.documentElement);
  if (!root) return;
  const parsed = themeDefinitionSchema.safeParse(theme);
  if (!parsed.success) return;
  clearAppliedTheme(root);
  const next = new Set<string>();
  for (const [group, values] of Object.entries(parsed.data)) {
    if (group === "themeId" || group === "name" || values === undefined) continue;
    if (group === "density") {
      root.style.setProperty("--gr-density", String(values));
      next.add("--gr-density");
      continue;
    }
    for (const [key, value] of Object.entries(values)) {
      const property = `--gr-${group}-${key}`;
      root.style.setProperty(property, String(value));
      next.add(property);
    }
  }
  appliedVariables.set(root, next);
}
