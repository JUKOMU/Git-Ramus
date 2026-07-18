import { z } from "zod";
import { themeDefinitionSchema } from "./theme";

const uuid = z.string().uuid();

export const hostInitSchema = z
  .object({
    type: z.literal("host:init"),
    sessionId: uuid,
    pluginId: z.string().min(1),
    sdkVersion: z.string().min(1),
    route: z
      .string()
      .regex(/^\/[a-z0-9/-]*$/u)
      .default("/")
  })
  .strict();

export const pluginReadySchema = z
  .object({
    type: z.literal("plugin:ready"),
    sessionId: uuid
  })
  .strict();

export const rpcRequestSchema = z
  .object({
    type: z.literal("rpc:request"),
    requestId: uuid,
    sessionId: uuid,
    method: z.string().min(1),
    params: z.unknown()
  })
  .strict();

export const rpcResultSchema = z.discriminatedUnion("ok", [
  z
    .object({
      type: z.literal("rpc:result"),
      requestId: uuid,
      sessionId: uuid,
      ok: z.literal(true),
      result: z.unknown()
    })
    .strict(),
  z
    .object({
      type: z.literal("rpc:result"),
      requestId: uuid,
      sessionId: uuid,
      ok: z.literal(false),
      error: z.unknown()
    })
    .strict()
]);

export const hostThemeChangedSchema = z
  .object({
    type: z.literal("host:theme-changed"),
    sessionId: uuid,
    theme: themeDefinitionSchema
  })
  .strict();

export const hostToPluginMessageSchema = z.union([
  hostInitSchema,
  rpcResultSchema,
  hostThemeChangedSchema
]);
export const pluginToHostMessageSchema = z.union([pluginReadySchema, rpcRequestSchema]);

// Route is optional at the wire boundary for schema-v1 plugins; the SDK normalizes it to "/".
export type HostInit = Omit<z.infer<typeof hostInitSchema>, "route"> & { route?: string };
export type HostThemeChanged = z.infer<typeof hostThemeChangedSchema>;
export type RpcRequest = z.infer<typeof rpcRequestSchema>;
export type RpcResult = z.infer<typeof rpcResultSchema>;
export type HostToPluginMessage = HostInit | HostThemeChanged | RpcResult;
export type PluginToHostMessage = z.infer<typeof pluginToHostMessageSchema>;
