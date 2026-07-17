import { z } from "zod";
import { errorEnvelopeSchema } from "./errors";

export const jobStatusSchema = z.enum(["queued", "running", "succeeded", "failed", "canceled"]);

export const jobSchema = z
  .object({
    id: z.string().uuid(),
    kind: z.string().min(1),
    title: z.string().min(1),
    status: jobStatusSchema,
    progress: z.number().min(0).max(1),
    cancelRequested: z.boolean(),
    createdAt: z.string().datetime(),
    updatedAt: z.string().datetime(),
    error: errorEnvelopeSchema.nullable()
  })
  .strict();

export type JobStatus = z.infer<typeof jobStatusSchema>;
export type Job = z.infer<typeof jobSchema>;
