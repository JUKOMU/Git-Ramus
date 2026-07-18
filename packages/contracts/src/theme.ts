import { z } from "zod";

const tokenKey = z.string().regex(/^[a-z][a-zA-Z0-9-]*$/u);
const tokenValue = z.union([z.string().min(1), z.number().finite(), z.boolean()]);
const tokenGroup = z.record(tokenKey, tokenValue).optional();

/** Host-validated design tokens. No CSS text, selectors, scripts, or other executable values are accepted. */
export const themeDefinitionSchema = z
  .object({
    themeId: z.string().regex(/^[a-z0-9]+(?:[.-][a-z0-9]+)+$/u),
    name: z.string().min(1).max(64).optional(),
    colors: tokenGroup,
    typography: tokenGroup,
    spacing: tokenGroup,
    shape: tokenGroup,
    elevation: tokenGroup,
    motion: tokenGroup,
    density: tokenGroup
  })
  .strict();

export type ThemeDefinition = z.infer<typeof themeDefinitionSchema>;
