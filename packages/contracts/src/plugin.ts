import { z } from "zod";

const safeRelativePath = z
  .string()
  .min(1)
  .refine((value) => !/^(?:[\\/]|[A-Za-z]:)/u.test(value), "path is absolute")
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

export const pluginManifestSchema = z
  .object({
    schemaVersion: z.literal(1),
    id: z.string().regex(/^[a-z0-9]+(?:[.-][a-z0-9]+)+$/u),
    name: z.string().min(1).max(64),
    version: z.string().regex(/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/u),
    publisher: z.string().regex(/^[a-z0-9-]+$/u),
    description: z.string().min(1).max(256),
    kind: z.enum(["builtin", "external"]),
    sdkVersion: z.string().min(1),
    entrypoints: z
      .object({
        ui: safeRelativePath
      })
      .strict(),
    contributions: z
      .object({
        navigation: z.array(navigationContributionSchema)
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
