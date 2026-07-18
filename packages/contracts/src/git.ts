import { z } from "zod";

const uuid = z.string().uuid();
const timestamp = z.string().datetime();
const nullableUuid = uuid.nullable();

export const projectSchema = z
  .object({
    id: uuid,
    name: z.string().min(1),
    rootPath: z.string().min(1),
    scanDepth: z.number().int().nonnegative(),
    excludePatterns: z.array(z.string().min(1)),
    createdAt: timestamp,
    updatedAt: timestamp
  })
  .strict();

export const workspaceSchema = z
  .object({
    id: uuid,
    name: z.string().min(1),
    rootPath: z.string().min(1).optional(),
    projectIds: z.array(uuid).default([]),
    createdAt: timestamp,
    updatedAt: timestamp
  })
  .strict();

export const repositorySchema = z
  .object({
    id: uuid,
    workspaceIds: z.array(uuid).default([]),
    canonicalPath: z.string().min(1),
    displayName: z.string().min(1),
    kind: z.enum(["normal", "bare", "worktree"]),
    remoteUrl: z.string().min(1).nullable().optional(),
    defaultBranch: z.string().min(1).nullable().optional(),
    createdAt: timestamp,
    updatedAt: timestamp
  })
  .strict();

export const changeEntrySchema = z
  .object({
    path: z.string().min(1),
    status: z.enum(["added", "modified", "deleted", "renamed", "untracked", "conflicted"]),
    oldPath: z.string().min(1).nullable().optional(),
    staged: z.boolean().optional(),
    additions: z.number().int().nonnegative().nullable().optional(),
    deletions: z.number().int().nonnegative().nullable().optional()
  })
  .strict();

export const repositorySnapshotSchema = z
  .object({
    id: uuid,
    repositoryId: uuid,
    branch: z.string().min(1).nullable(),
    headSha: z.string().min(1).nullable(),
    isDirty: z.boolean(),
    ahead: z.number().int().nonnegative(),
    behind: z.number().int().nonnegative(),
    changes: z.array(changeEntrySchema),
    upstream: z
      .object({
        branch: z.string().min(1).nullable(),
        ahead: z.number().int().nonnegative(),
        behind: z.number().int().nonnegative()
      })
      .strict()
      .nullable(),
    summary: z
      .object({
        total: z.number().int().nonnegative(),
        added: z.number().int().nonnegative(),
        modified: z.number().int().nonnegative(),
        deleted: z.number().int().nonnegative(),
        untracked: z.number().int().nonnegative()
      })
      .strict(),
    capturedAt: timestamp
  })
  .strict();

export const identityProfileSchema = z
  .object({
    id: uuid,
    displayName: z.string().min(1),
    email: z.string().email(),
    gpgFormat: z.enum(["openpgp", "ssh", "x509", "none"]).nullable().optional(),
    signingKey: z.string().min(1).nullable().optional(),
    isGlobal: z.boolean().optional(),
    signCommits: z.boolean(),
    signTags: z.boolean(),
    createdAt: timestamp,
    updatedAt: timestamp
  })
  .strict();

export const effectiveIdentitySchema = z
  .object({
    profile: identityProfileSchema.optional(),
    profileId: nullableUuid.optional(),
    displayName: z.string().min(1).optional(),
    email: z.string().email().optional(),
    gpgFormat: z.enum(["openpgp", "ssh", "x509", "none"]).nullable().optional(),
    signingKey: z.string().min(1).nullable().optional(),
    source: z.enum(["global", "repository", "workspace", "default"]),
    repositoryId: nullableUuid.optional()
  })
  .strict()
  .refine(
    (value) =>
      value.profile !== undefined || (value.displayName !== undefined && value.email !== undefined),
    {
      message: "effective identity details are required"
    }
  );

export const operationResponseSchema = z
  .object({
    operationId: uuid,
    status: z.enum(["accepted", "completed", "failed"]),
    result: z.unknown().optional()
  })
  .strict();

export const repositoryOperationResponseSchema = z
  .object({
    operationId: uuid,
    repositoryId: uuid,
    status: z.enum(["accepted", "completed", "failed"]),
    snapshot: repositorySnapshotSchema.optional()
  })
  .strict();

// Alias retained for callers that use the Git-specific name.
export const gitOperationResponseSchema = operationResponseSchema;

export type Project = z.infer<typeof projectSchema>;
export type Workspace = z.infer<typeof workspaceSchema>;
export type Repository = z.infer<typeof repositorySchema>;
export type RepositorySnapshot = z.infer<typeof repositorySnapshotSchema>;
export type ChangeEntry = z.infer<typeof changeEntrySchema>;
export type IdentityProfile = z.infer<typeof identityProfileSchema>;
export type EffectiveIdentity = z.infer<typeof effectiveIdentitySchema>;
export type OperationResponse = z.infer<typeof operationResponseSchema>;
export type RepositoryOperationResponse = z.infer<typeof repositoryOperationResponseSchema>;
export type GitOperationResponse = OperationResponse;
