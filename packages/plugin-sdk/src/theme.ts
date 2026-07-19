import { themeDefinitionSchema, type ThemeDefinition } from "@git-ramus/contracts";

interface AppliedTheme {
  variables: Set<string>;
  owner: object | undefined;
}

const appliedThemes = new WeakMap<HTMLElement, AppliedTheme>();
const TOKEN_KEYS = {
  colors: [
    "background",
    "surface",
    "surfaceRaised",
    "text",
    "textMuted",
    "border",
    "primary",
    "secondary",
    "accent",
    "success",
    "warning",
    "danger",
    "focusRing"
  ],
  typography: ["fontFamily", "fontSize", "lineHeight", "fontWeight", "letterSpacing"],
  spacing: ["unit", "xs", "sm", "md", "lg", "xl"],
  shape: ["radius", "radiusSm", "radiusMd", "radiusLg"],
  elevation: ["none", "sm", "md", "lg", "level1", "level2", "level3"],
  motion: ["durationFast", "durationNormal", "durationSlow", "easing"]
} as const;
const LENGTH_TOKENS = new Set([
  "typography.fontSize",
  "typography.letterSpacing",
  ...TOKEN_KEYS.spacing.map((key) => `spacing.${key}`),
  ...TOKEN_KEYS.shape.map((key) => `shape.${key}`)
]);

export function clearAppliedTheme(documentRoot?: HTMLElement, owner?: object): void {
  const root =
    documentRoot ?? (typeof document === "undefined" ? undefined : document.documentElement);
  if (!root) return;
  const previous = appliedThemes.get(root);
  if (!previous) return;
  if (owner !== undefined && previous.owner !== owner) return;
  for (const property of previous.variables) root.style.removeProperty(property);
  appliedThemes.delete(root);
}

export function applyThemeToDocument(
  theme: ThemeDefinition,
  documentRoot?: HTMLElement,
  owner?: object
): void {
  const root =
    documentRoot ?? (typeof document === "undefined" ? undefined : document.documentElement);
  if (!root) return;
  const parsed = themeDefinitionSchema.safeParse(theme);
  if (!parsed.success) return;
  clearAppliedTheme(root);
  const next = new Set<string>();
  for (const [group, keys] of Object.entries(TOKEN_KEYS)) {
    const values = parsed.data[group as keyof typeof TOKEN_KEYS] as
      Record<string, string | number | undefined> | undefined;
    if (values === undefined) continue;
    for (const key of keys) {
      const value = values[key];
      if (value === undefined) continue;
      const property = `--gr-${group}-${key}`;
      const token = `${group}.${key}`;
      root.style.setProperty(
        property,
        typeof value === "number" && LENGTH_TOKENS.has(token) ? `${value}px` : String(value)
      );
      next.add(property);
    }
  }
  if (parsed.data.density !== undefined) {
    root.style.setProperty("--gr-density", parsed.data.density);
    next.add("--gr-density");
  }
  appliedThemes.set(root, { variables: next, owner });
}
