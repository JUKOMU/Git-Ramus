import { z } from "zod";

const unsafeManifestText = /[<>;{}]|url\s*\(|@import|javascript\s*:|expression\s*\(/iu;
const safeManifestText = (maximum: number) =>
  z
    .string()
    .min(1)
    .max(maximum)
    .refine((value) => value.trim().length > 0, "text must not be blank")
    .refine(
      (value) =>
        !unsafeManifestText.test(value) &&
        !Array.from(value).some((character) => {
          const code = character.charCodeAt(0);
          return code < 0x20 || code === 0x7f;
        }),
      "unsafe manifest text"
    );

export const safeRelativePath = z
  .string()
  .min(1)
  .refine((value) => !/^(?:[\\/]|[A-Za-z]:)/u.test(value), "path is absolute")
  .refine((value) => !/^[A-Za-z][A-Za-z0-9+.-]*:/u.test(value), "path has a URL scheme")
  .refine(
    (value) =>
      !Array.from(value).some((character) => {
        const code = character.charCodeAt(0);
        return code < 0x20 || code === 0x7f;
      }),
    "path contains control characters"
  )
  .refine((value) => !value.split(/[\\/]/u).includes(".."), "path traverses its plugin root");

export const permissionRequestSchema = z
  .object({
    capability: z.string().regex(/^[a-z][a-z0-9.-]*:[a-z][a-z0-9.-]*$/u),
    resources: z.array(z.string().min(1)).min(1)
  })
  .strict();

export const navigationContributionSchema = z
  .object({
    id: z.string().regex(/^[a-z][a-z0-9-]*$/u),
    label: z.string().min(1).max(64),
    route: z.string().regex(/^\/[a-z0-9/-]*$/u),
    icon: z.string().regex(/^[a-z][a-z0-9-]*$/u)
  })
  .strict();

const themeIdSchema = z.string().regex(/^[a-z0-9]+(?:[.-][a-z0-9]+)+$/u);
export const themeContributionSchema = z
  .object({
    themeId: themeIdSchema,
    definition: safeRelativePath.optional(),
    definitionPath: safeRelativePath.optional()
  })
  .strict()
  .refine((value) => value.definition !== undefined || value.definitionPath !== undefined, {
    message: "theme definition path is required"
  })
  .refine(
    (value) =>
      value.definition === undefined ||
      value.definitionPath === undefined ||
      value.definition === value.definitionPath,
    {
      message: "theme definition and definitionPath must match"
    }
  );

export const pluginManifestSchema = z
  .object({
    schemaVersion: z.literal(1),
    id: z.string().regex(/^[a-z0-9]+(?:[.-][a-z0-9]+)+$/u),
    name: safeManifestText(64),
    version: z.string().regex(/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/u),
    publisher: z.string().regex(/^[a-z0-9-]+$/u),
    description: safeManifestText(256),
    kind: z.enum(["builtin", "external"]),
    sdkVersion: z.string().min(1),
    entrypoints: z
      .object({
        ui: safeRelativePath
      })
      .strict(),
    contributions: z
      .object({
        navigation: z.array(navigationContributionSchema),
        theme: themeContributionSchema.optional()
      })
      .strict(),
    permissions: z.array(permissionRequestSchema)
  })
  .strict();

export const pluginDescriptorSchema = z
  .object({
    manifest: pluginManifestSchema,
    uiUrl: z
      .string()
      .regex(
        /^(?:git-ramus-plugin:\/\/localhost|https?:\/\/git-ramus-plugin\.localhost)\/[a-z0-9.-]+\/ui\.html$/u
      )
  })
  .strict();

export type PermissionRequest = z.infer<typeof permissionRequestSchema>;
export type PluginManifest = z.infer<typeof pluginManifestSchema>;
export type PluginDescriptor = z.infer<typeof pluginDescriptorSchema>;
