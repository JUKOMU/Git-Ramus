import { z } from "zod";

const uuid = z.string().uuid();

export const hostInitSchema = z
  .object({
    type: z.literal("host:init"),
    sessionId: uuid,
    pluginId: z.string().min(1),
    sdkVersion: z.string().min(1)
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

export const hostToPluginMessageSchema = z.union([hostInitSchema, rpcResultSchema]);
export const pluginToHostMessageSchema = z.union([pluginReadySchema, rpcRequestSchema]);

export type HostInit = z.infer<typeof hostInitSchema>;
export type RpcRequest = z.infer<typeof rpcRequestSchema>;
export type RpcResult = z.infer<typeof rpcResultSchema>;
export type HostToPluginMessage = z.infer<typeof hostToPluginMessageSchema>;
export type PluginToHostMessage = z.infer<typeof pluginToHostMessageSchema>;
