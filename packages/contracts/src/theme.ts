import { z } from "zod";

const themeId = z.string().regex(/^[a-z0-9]+(?:[.-][a-z0-9]+)+$/u);
const unsafeCss = /[<>;{}]|url\s*\(|@import|javascript\s*:|expression\s*\(/iu;
const safeText = z
  .string()
  .min(1)
  .max(256)
  .refine((value) => !unsafeCss.test(value), "unsafe CSS value");
const safeNumber = z.number().finite();
const cssLength = z.union([
  safeNumber,
  safeText.refine(
    (value) =>
      /^(?:0|[-+]?\d*\.?\d+(?:px|rem|em|%|vh|vw|vmin|vmax)?)$/u.test(value) ||
      /^var\(--[a-z0-9-]+\)$/u.test(value),
    "invalid length"
  )
]);
const cssColor = z
  .string()
  .regex(
    /^(?:#[0-9a-f]{3,8}|(?:rgb|hsl)a?\([^)]*\)|transparent|currentColor|[a-z]+|var\(--[a-z0-9-]+\))$/iu,
    "invalid color"
  )
  .refine((value) => !unsafeCss.test(value), "unsafe color");
const tokenObject = <T extends z.ZodType>(shape: Record<string, T>) =>
  z.object(shape).strict().partial();

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
const typography = tokenObject({
  fontFamily: safeText,
  fontSize: cssLength,
  lineHeight: z.union([safeNumber, cssLength]),
  fontWeight: z.union([safeNumber.int().min(100).max(900), safeText]),
  letterSpacing: cssLength
});
const spacing = tokenObject({
  unit: cssLength,
  xs: cssLength,
  sm: cssLength,
  md: cssLength,
  lg: cssLength,
  xl: cssLength
});
const shape = tokenObject({
  radius: cssLength,
  radiusSm: cssLength,
  radiusMd: cssLength,
  radiusLg: cssLength
});
const elevation = tokenObject({
  none: safeText,
  sm: safeText,
  md: safeText,
  lg: safeText,
  level1: safeText,
  level2: safeText,
  level3: safeText
});
const motion = tokenObject({
  durationFast: safeText,
  durationNormal: safeText,
  durationSlow: safeText,
  easing: safeText
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

export type ThemeDefinition = z.infer<typeof themeDefinitionSchema>;
