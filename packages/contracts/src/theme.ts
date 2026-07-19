import { z } from "zod";

const themeId = z.string().regex(/^[a-z0-9]+(?:[.-][a-z0-9]+)+$/u);
const unsafeCss = /[<>;{}]|url\s*\(|@import|javascript\s*:|expression\s*\(/iu;
const safeText = z
  .string()
  .trim()
  .min(1)
  .max(64)
  .refine((value) => !unsafeCss.test(value), "unsafe theme text");
const cssColor = z
  .string()
  .regex(
    /^(?:#[0-9a-f]{3,4}|#[0-9a-f]{6}(?:[0-9a-f]{2})?|transparent|currentColor)$/iu,
    "invalid color"
  );

type ThemeLength = number | string;

function parseLength(value: string): { number: number; unit: string } | null {
  if (unsafeCss.test(value) || !/^-?(?:\d+(?:\.\d*)?|\.\d+)(?:px|rem|em|%)?$/u.test(value)) {
    return null;
  }
  const match = /^(-?(?:\d+(?:\.\d*)?|\.\d+))(.*)$/u.exec(value);
  if (match === null) return null;
  const number = Number(match[1]);
  return Number.isFinite(number) ? { number, unit: match[2] ?? "" } : null;
}

function lengthWithinBounds(
  value: ThemeLength,
  minimum: number,
  maximum: number,
  allowNegative: boolean
): boolean {
  const parsed = typeof value === "number" ? { number: value, unit: "" } : parseLength(value);
  if (parsed === null || !Number.isFinite(parsed.number)) return false;
  if (!allowNegative && parsed.number < 0) return false;
  if (parsed.unit === "" || parsed.unit === "px") {
    return parsed.number >= minimum && parsed.number <= maximum;
  }
  if (parsed.unit === "rem" || parsed.unit === "em") {
    return parsed.number >= minimum / 16 && parsed.number <= maximum / 16;
  }
  return minimum >= 0 && parsed.unit === "%" && parsed.number >= 0 && parsed.number <= 100;
}

const boundedLength = (minimum: number, maximum: number, allowNegative = false) =>
  z
    .union([z.number().finite(), z.string()])
    .refine(
      (value) => lengthWithinBounds(value, minimum, maximum, allowNegative),
      "theme length is outside the safe range"
    );

const tokenObject = <T extends z.ZodRawShape>(shape: T) => z.object(shape).strict().partial();

const colors = tokenObject({
  background: cssColor,
  surface: cssColor,
  surfaceRaised: cssColor,
  text: cssColor,
  textMuted: cssColor,
  border: cssColor,
  primary: cssColor,
  secondary: cssColor,
  accent: cssColor,
  success: cssColor,
  warning: cssColor,
  danger: cssColor,
  focusRing: cssColor
});
const fontFamily = z
  .string()
  .min(1)
  .max(128)
  .refine((value) => !unsafeCss.test(value), "unsafe font family")
  .regex(/^[\p{L}\p{N}\s,_'"-]+$/u, "invalid font family");
const typography = tokenObject({
  fontFamily,
  fontSize: boundedLength(8, 72),
  lineHeight: boundedLength(1, 3),
  fontWeight: z.union([z.number().int().min(100).max(900), z.enum(["normal", "bold"])]),
  letterSpacing: boundedLength(-4, 16, true)
});
const spacing = tokenObject({
  unit: boundedLength(0, 128),
  xs: boundedLength(0, 128),
  sm: boundedLength(0, 128),
  md: boundedLength(0, 128),
  lg: boundedLength(0, 128),
  xl: boundedLength(0, 128)
});
const shape = tokenObject({
  radius: boundedLength(0, 64),
  radiusSm: boundedLength(0, 64),
  radiusMd: boundedLength(0, 64),
  radiusLg: boundedLength(0, 64)
});
const shadow = z
  .string()
  .min(1)
  .max(128)
  .refine((value) => !unsafeCss.test(value), "unsafe elevation")
  .regex(/^[A-Za-z0-9\s.#(),%_-]+$/u, "invalid elevation");
const elevation = tokenObject({
  none: shadow,
  sm: shadow,
  md: shadow,
  lg: shadow,
  level1: shadow,
  level2: shadow,
  level3: shadow
});
const duration = z.string().refine((value) => {
  if (unsafeCss.test(value)) return false;
  const match = /^(\d+(?:\.\d+)?)(ms|s)$/u.exec(value);
  if (match === null) return false;
  const number = Number(match[1]);
  const milliseconds = match[2] === "s" ? number * 1_000 : number;
  return Number.isFinite(milliseconds) && milliseconds >= 0 && milliseconds <= 2_000;
}, "invalid motion duration");
const motion = tokenObject({
  durationFast: duration,
  durationNormal: duration,
  durationSlow: duration,
  easing: z.enum(["linear", "ease", "ease-in", "ease-out", "ease-in-out"])
});

export const themeDefinitionSchema = z
  .object({
    themeId,
    name: safeText.optional(),
    colors: colors.optional(),
    typography: typography.optional(),
    spacing: spacing.optional(),
    shape: shape.optional(),
    elevation: elevation.optional(),
    motion: motion.optional(),
    density: z.enum(["comfortable", "compact"]).optional()
  })
  .strict();

export const themeMetadataSchema = z
  .object({
    themeId,
    name: safeText,
    pluginId: themeId,
    version: z.string().regex(/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/u),
    density: z.enum(["comfortable", "compact"])
  })
  .strict();

export const themeCatalogSchema = z
  .object({
    themes: z.array(themeMetadataSchema)
  })
  .strict();

export const themeStateSchema = z
  .object({
    activeThemeId: themeId,
    theme: themeDefinitionSchema
  })
  .strict()
  .refine((state) => state.activeThemeId === state.theme.themeId, {
    message: "active theme id must match the definition"
  });

export const themeActivateRequestSchema = z
  .object({
    themeId
  })
  .strict();

export const themeActivationResponseSchema = themeStateSchema;

export type ThemeDefinition = z.infer<typeof themeDefinitionSchema>;
export type ThemeMetadata = z.infer<typeof themeMetadataSchema>;
export type ThemeCatalog = z.infer<typeof themeCatalogSchema>;
export type ThemeState = z.infer<typeof themeStateSchema>;
export type ThemeActivateRequest = z.infer<typeof themeActivateRequestSchema>;
export type ThemeActivationResponse = ThemeState;
