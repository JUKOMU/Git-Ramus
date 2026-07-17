import { z } from "zod";

export const recoveryActionSchema = z
  .object({
    id: z.string().regex(/^[a-z][a-z0-9-]*$/u),
    label: z.string().min(1).max(64),
    kind: z.enum(["retry", "openSettings", "reauthorize", "resolveConflict", "exportDiagnostics"])
  })
  .strict();

export const errorEnvelopeSchema = z
  .object({
    code: z.string().regex(/^[a-z][a-z0-9-]*(?:\.[a-z0-9-]+)+$/u),
    category: z.enum([
      "validation",
      "userActionRequired",
      "retryable",
      "partialResult",
      "internalFatal"
    ]),
    message: z.string().min(1),
    operationId: z.string().uuid().nullable(),
    pluginId: z.string().nullable(),
    resourceId: z.string().nullable(),
    failedStep: z.string().nullable(),
    retryable: z.boolean(),
    retryAfterMs: z.number().int().nonnegative().nullable(),
    recoveryActions: z.array(recoveryActionSchema),
    details: z.record(z.string(), z.unknown()).nullable()
  })
  .strict();

export type ErrorEnvelope = z.infer<typeof errorEnvelopeSchema>;
export type RecoveryAction = z.infer<typeof recoveryActionSchema>;
