import { z } from "zod";

const uuid = z.string().uuid();
const timestamp = z.string().datetime();
const nullableText = z.string().nullable();
const nullableUuid = uuid.nullable();
const nonnegativeInteger = z.number().int().nonnegative();
const contextShape = {
  projectId: nullableUuid.optional(),
  workspaceId: nullableUuid.optional()
};

function hasExactlyOneContext(value: {
  projectId?: string | null | undefined;
  workspaceId?: string | null | undefined;
}) {
  return (value.projectId != null) !== (value.workspaceId != null);
}

export const repositoryRelativePathSchema = z
  .string()
  .min(1)
  .max(4 * 1024)
  .refine((value) => !value.includes("\0"), "path contains a NUL byte")
  .refine((value) => !/^[\\/]/u.test(value), "path is absolute")
  .refine((value) => !/^[A-Za-z]:(?:[\\/]|$)/u.test(value), "path has a drive prefix")
  .refine((value) => {
    const normalized = value.replaceAll("\\", "/");
    return normalized
      .split("/")
      .every((part) => part !== "" && part !== "." && part !== ".." && !part.includes(":"));
  }, "path is not confined to the repository");

export const projectSchema = z
  .object({
    id: uuid,
    rootPath: z.string().min(1),
    name: z.string().min(1).max(256),
    scanDepth: z.number().int().min(0).max(64),
    excludePatterns: z.array(z.string().min(1).max(512)).max(256),
    createdAt: timestamp,
    updatedAt: timestamp
  })
  .strict();

export const workspaceSchema = z
  .object({
    id: uuid,
    name: z.string().min(1).max(256),
    createdAt: timestamp,
    updatedAt: timestamp
  })
  .strict();

export const repositorySchema = z
  .object({
    id: uuid,
    canonicalPath: z.string().min(1),
    displayName: z.string().min(1),
    kind: z.enum(["normal", "bare", "worktree"]),
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
    additions: nonnegativeInteger.nullable().optional(),
    deletions: nonnegativeInteger.nullable().optional()
  })
  .strict();

// Public Task 1 asynchronous-operation payload. Keep this stable for existing SDK consumers.
export const repositorySnapshotSchema = z
  .object({
    id: uuid,
    repositoryId: uuid,
    branch: z.string().min(1).nullable(),
    headSha: z.string().min(1).nullable(),
    isDirty: z.boolean(),
    ahead: nonnegativeInteger,
    behind: nonnegativeInteger,
    changes: z.array(changeEntrySchema),
    upstream: z
      .object({
        remote: z.string().min(1).nullable().optional(),
        branch: z.string().min(1).nullable(),
        ahead: nonnegativeInteger,
        behind: nonnegativeInteger
      })
      .strict()
      .nullable(),
    summary: z
      .object({
        total: nonnegativeInteger,
        added: nonnegativeInteger,
        modified: nonnegativeInteger,
        deleted: nonnegativeInteger,
        untracked: nonnegativeInteger,
        staged: nonnegativeInteger.optional(),
        unstaged: nonnegativeInteger.optional(),
        conflicted: nonnegativeInteger.optional()
      })
      .strict(),
    capturedAt: timestamp
  })
  .strict();

// Exact camelCase DTO serialized by the Rust persistence/service layer in Task 6.
export const persistedRepositorySnapshotSchema = z
  .object({
    id: uuid,
    repositoryId: uuid,
    capturedAt: timestamp,
    headOid: nullableText,
    branch: nullableText,
    upstream: nullableText,
    ahead: nonnegativeInteger,
    behind: nonnegativeInteger,
    dirty: z.boolean(),
    stagedCount: nonnegativeInteger,
    unstagedCount: nonnegativeInteger,
    untrackedCount: nonnegativeInteger,
    conflictedCount: nonnegativeInteger,
    refreshErrorSummary: nullableText
  })
  .strict();

export const changeKindSchema = z.enum([
  "added",
  "modified",
  "deleted",
  "renamed",
  "copied",
  "typeChanged",
  "untracked",
  "conflicted",
  "unknown"
]);

export const parsedChangeEntrySchema = z
  .object({
    path: z.string().min(1),
    originalPath: nullableText,
    kind: changeKindSchema,
    staged: z.boolean(),
    unstaged: z.boolean(),
    conflicted: z.boolean(),
    binary: z.boolean(),
    old: nullableText,
    new: nullableText,
    oldPath: nullableText,
    newPath: nullableText,
    status: z.string().length(2),
    indexStatus: z.string().length(1).nullable(),
    worktreeStatus: z.string().length(1).nullable(),
    additions: nonnegativeInteger.nullable(),
    deletions: nonnegativeInteger.nullable()
  })
  .strict();

export const parsedRepositorySnapshotSchema = z
  .object({
    branch: nullableText,
    upstream: nullableText,
    headOid: nullableText,
    headSha: nullableText,
    ahead: nonnegativeInteger,
    behind: nonnegativeInteger,
    changes: z.array(parsedChangeEntrySchema),
    dirty: z.boolean(),
    isDirty: z.boolean(),
    stagedCount: nonnegativeInteger,
    unstagedCount: nonnegativeInteger,
    untrackedCount: nonnegativeInteger,
    conflictedCount: nonnegativeInteger,
    totalCount: nonnegativeInteger,
    detached: z.boolean()
  })
  .strict();

export const diffFileSchema = z
  .object({
    path: z.string().min(1),
    oldPath: nullableText,
    newPath: nullableText,
    binary: z.boolean(),
    additions: nonnegativeInteger.nullable(),
    deletions: nonnegativeInteger.nullable(),
    old: nullableText,
    new: nullableText
  })
  .strict();

export const diffSummarySchema = z
  .object({
    files: z.array(diffFileSchema),
    changes: z.array(diffFileSchema),
    entries: z.array(diffFileSchema),
    binary: z.boolean(),
    additions: nonnegativeInteger,
    deletions: nonnegativeInteger
  })
  .strict();

export const diffContentUnavailableReasonSchema = z.enum([
  "binary",
  "untrustedRepository",
  "untrackedContentUnavailable",
  "nonUtf8Content",
  "outputLimit"
]);

const gpgFormat = z.enum(["openpgp", "ssh", "x509"]);
const gitEmail = z
  .string()
  .min(3)
  .max(320)
  .refine((value) => {
    if (/\s|\p{Cc}/u.test(value)) {
      return false;
    }
    const separator = value.indexOf("@");
    if (separator <= 0 || separator !== value.lastIndexOf("@") || separator === value.length - 1) {
      return false;
    }
    const local = value.slice(0, separator);
    const domain = value.slice(separator + 1);
    return (
      !local.startsWith(".") &&
      !local.endsWith(".") &&
      !domain.startsWith(".") &&
      !domain.endsWith(".")
    );
  }, "Git user email has an invalid shape");
const identityFields = {
  displayName: z.string().min(1).max(256),
  userName: z.string().min(1).max(256),
  userEmail: gitEmail,
  gpgFormat: gpgFormat.nullable(),
  signingKey: z
    .string()
    .min(1)
    .max(4 * 1024)
    .nullable(),
  signCommits: z.boolean(),
  signTags: z.boolean()
};

export const identityProfileSchema = z
  .object({
    id: uuid,
    ...identityFields,
    createdAt: timestamp,
    updatedAt: timestamp
  })
  .strict();

export const identityDriftFieldSchema = z
  .object({
    key: z.string().min(1),
    expected: z.array(z.string()),
    actual: z.array(z.string())
  })
  .strict();

export const identityDriftSchema = z
  .object({
    fields: z.array(identityDriftFieldSchema)
  })
  .strict();

export const effectiveIdentitySchema = z
  .object({
    repositoryId: uuid,
    profileId: nullableUuid,
    profile: identityProfileSchema.nullable(),
    source: z.enum([
      "globalProfile",
      "repositoryProfile",
      "selectedProfile",
      "externalGlobal",
      "externalLocal"
    ]),
    displayName: z.string().min(1),
    userName: z.string().min(1),
    userEmail: z.string().min(1),
    gpgFormat: z.string().min(1).nullable(),
    signingKey: z.string().min(1).nullable(),
    signCommits: z.boolean(),
    signTags: z.boolean(),
    drift: identityDriftSchema.nullable()
  })
  .strict();

export const trustSchema = z
  .object({
    repositoryId: uuid,
    trustedAt: timestamp,
    trustVersion: nonnegativeInteger
  })
  .strict();

export const identityBindingSchema = z
  .object({
    repositoryId: uuid,
    identityProfileId: uuid,
    managed: z.boolean(),
    boundAt: timestamp
  })
  .strict();

export const gitContextRequestSchema = z
  .object(contextShape)
  .strict()
  .refine(hasExactlyOneContext, "exactly one projectId or workspaceId is required");

export const projectListResponseSchema = z.object({ projects: z.array(projectSchema) }).strict();

// Project roots are selected by the trusted host. Plugins can request the picker, but can never
// supply a path or project metadata across the RPC boundary.
export const projectCreateRequestSchema = z.object({}).strict();

export const projectUpdateScanRulesRequestSchema = z
  .object({
    projectId: uuid,
    scanDepth: z.number().int().min(0).max(64).nullable().optional(),
    excludePatterns: z.array(z.string().min(1).max(512)).max(256).nullable().optional()
  })
  .strict();

export const projectScanRequestSchema = z.object({ projectId: uuid }).strict();

export const workspaceListResponseSchema = z
  .object({ workspaces: z.array(workspaceSchema) })
  .strict();

export const workspaceCreateRequestSchema = z.object({ name: z.string().min(1).max(256) }).strict();

export const workspaceRequestSchema = z.object({ workspaceId: uuid }).strict();

export const workspaceUpdateMembershipRequestSchema = z
  .object({ workspaceId: uuid, projectIds: z.array(uuid) })
  .strict();

export const workspaceMembershipResponseSchema = z.array(uuid);
export const workspaceDeleteRequestSchema = z.object({ workspaceId: uuid }).strict();

export const repositoryRequestSchema = z
  .object({ ...contextShape, repositoryId: uuid })
  .strict()
  .refine(hasExactlyOneContext, "exactly one projectId or workspaceId is required");

export const repositoryDiffRequestSchema = z
  .object({
    ...contextShape,
    repositoryId: uuid,
    paths: z.array(repositoryRelativePathSchema).default([]),
    staged: z.boolean().default(false)
  })
  .strict()
  .refine(hasExactlyOneContext, "exactly one projectId or workspaceId is required");

export const repositoryStageRequestSchema = z
  .object({
    ...contextShape,
    repositoryId: uuid,
    paths: z.array(repositoryRelativePathSchema).default([]),
    all: z.boolean().default(false)
  })
  .strict()
  .refine(hasExactlyOneContext, "exactly one projectId or workspaceId is required");

export const repositoryUnstageRequestSchema = z
  .object({
    ...contextShape,
    repositoryId: uuid,
    paths: z.array(repositoryRelativePathSchema)
  })
  .strict()
  .refine(hasExactlyOneContext, "exactly one projectId or workspaceId is required");

export const repositoryCommitRequestSchema = z
  .object({
    ...contextShape,
    repositoryId: uuid,
    message: z
      .string()
      .trim()
      .min(1)
      .max(128 * 1024),
    identityProfileId: nullableUuid.optional()
  })
  .strict()
  .refine(hasExactlyOneContext, "exactly one projectId or workspaceId is required");

const identityRequestFields = {
  displayName: z.string().min(1).max(256),
  userName: z.string().min(1).max(256),
  userEmail: gitEmail,
  gpgFormat: z.enum(["openpgp", "ssh", "x509", "none"]).nullable().optional(),
  signingKey: z
    .string()
    .max(4 * 1024)
    .nullable()
    .optional(),
  signCommits: z.boolean().default(false),
  signTags: z.boolean().default(false)
};

export const identityCreateRequestSchema = z.object(identityRequestFields).strict();
export const identityUpdateRequestSchema = z
  .object({ profileId: uuid, ...identityRequestFields })
  .strict();
export const identityProfileRequestSchema = z.object({ profileId: uuid }).strict();

export const repositoryIdentityBindRequestSchema = z
  .object({ ...contextShape, repositoryId: uuid, identityProfileId: uuid })
  .strict()
  .refine(hasExactlyOneContext, "exactly one projectId or workspaceId is required");

export const repositoryIdentityRequestSchema = repositoryRequestSchema;

export const repositoryScanRecordSchema = z
  .object({
    repository: repositorySchema,
    snapshot: persistedRepositorySnapshotSchema.nullable(),
    changes: parsedRepositorySnapshotSchema.nullable(),
    error: nullableText
  })
  .strict();

export const repositoryScanFailureSchema = z
  .object({ path: z.string().min(1), error: z.string().min(1) })
  .strict();

export const scanProgressRecordSchema = z
  .object({
    index: nonnegativeInteger,
    total: nonnegativeInteger,
    repositoryId: uuid,
    completed: z.boolean(),
    error: nullableText
  })
  .strict();

export const scanProjectResultSchema = z
  .object({
    projectId: uuid,
    repositories: z.array(repositoryScanRecordSchema),
    failures: z.array(repositoryScanFailureSchema),
    total: nonnegativeInteger,
    completed: nonnegativeInteger,
    failed: nonnegativeInteger,
    discoveryFailed: nonnegativeInteger,
    progress: z.array(scanProgressRecordSchema)
  })
  .strict();

export const overviewRepositorySchema = z
  .object({ repository: repositorySchema, snapshot: persistedRepositorySnapshotSchema.nullable() })
  .strict();

export const overviewSchema = z
  .object({
    context: gitContextRequestSchema,
    repositories: z.array(overviewRepositorySchema),
    repositoryCount: nonnegativeInteger,
    dirtyCount: nonnegativeInteger,
    stagedCount: nonnegativeInteger,
    unstagedCount: nonnegativeInteger,
    untrackedCount: nonnegativeInteger,
    conflictedCount: nonnegativeInteger,
    branches: z.array(z.string().min(1))
  })
  .strict();

export const changesResultSchema = z
  .object({
    repositoryId: uuid,
    snapshot: persistedRepositorySnapshotSchema,
    changes: z.array(parsedChangeEntrySchema)
  })
  .strict();

export const diffResultSchema = z
  .object({
    repositoryId: uuid,
    staged: z.boolean(),
    summary: diffSummarySchema,
    patch: z.string().nullable(),
    truncated: z.boolean(),
    contentUnavailableReason: diffContentUnavailableReasonSchema.nullable()
  })
  .strict();

export const writeResultSchema = z
  .object({
    repositoryId: uuid,
    snapshot: persistedRepositorySnapshotSchema.nullable(),
    output: nullableText
  })
  .strict();

export const trustResponseSchema = z.object({ trust: trustSchema }).strict();
export const trustStatusResponseSchema = z.object({ trusted: z.boolean() }).strict();

export const identityListResponseSchema = z
  .object({ identities: z.array(identityProfileSchema), globalIdentityProfileId: nullableUuid })
  .strict();

// Task 1's asynchronous operation contract remains available for existing SDK consumers. The
// Task 5 Rust Git commands use `writeResultSchema` above instead.
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

export const gitOperationResponseSchema = operationResponseSchema;
export const queryContextSchema = gitContextRequestSchema;

export type Project = z.infer<typeof projectSchema>;
export type Workspace = z.infer<typeof workspaceSchema>;
export type Repository = z.infer<typeof repositorySchema>;
export type RepositorySnapshot = z.infer<typeof repositorySnapshotSchema>;
export type PersistedRepositorySnapshot = z.infer<typeof persistedRepositorySnapshotSchema>;
export type ParsedRepositorySnapshot = z.infer<typeof parsedRepositorySnapshotSchema>;
export type ChangeEntry = z.infer<typeof changeEntrySchema>;
export type ParsedChangeEntry = z.infer<typeof parsedChangeEntrySchema>;
export type DiffFile = z.infer<typeof diffFileSchema>;
export type DiffSummary = z.infer<typeof diffSummarySchema>;
export type DiffContentUnavailableReason = z.infer<typeof diffContentUnavailableReasonSchema>;
export type IdentityProfile = z.infer<typeof identityProfileSchema>;
export type EffectiveIdentity = z.infer<typeof effectiveIdentitySchema>;
export type Trust = z.infer<typeof trustSchema>;
export type IdentityBinding = z.infer<typeof identityBindingSchema>;
export type GitContextRequest = z.infer<typeof gitContextRequestSchema>;
export type ProjectListResponse = z.infer<typeof projectListResponseSchema>;
export type ProjectCreateRequest = z.infer<typeof projectCreateRequestSchema>;
export type ProjectUpdateScanRulesRequest = z.infer<typeof projectUpdateScanRulesRequestSchema>;
export type ProjectScanRequest = z.infer<typeof projectScanRequestSchema>;
export type WorkspaceListResponse = z.infer<typeof workspaceListResponseSchema>;
export type WorkspaceCreateRequest = z.infer<typeof workspaceCreateRequestSchema>;
export type WorkspaceRequest = z.infer<typeof workspaceRequestSchema>;
export type WorkspaceUpdateMembershipRequest = z.infer<
  typeof workspaceUpdateMembershipRequestSchema
>;
export type WorkspaceDeleteRequest = z.infer<typeof workspaceDeleteRequestSchema>;
export type RepositoryRequest = z.infer<typeof repositoryRequestSchema>;
export type RepositoryDiffRequest = z.infer<typeof repositoryDiffRequestSchema>;
export type RepositoryStageRequest = z.infer<typeof repositoryStageRequestSchema>;
export type RepositoryUnstageRequest = z.infer<typeof repositoryUnstageRequestSchema>;
export type RepositoryCommitRequest = z.infer<typeof repositoryCommitRequestSchema>;
export type IdentityCreateRequest = z.infer<typeof identityCreateRequestSchema>;
export type IdentityUpdateRequest = z.infer<typeof identityUpdateRequestSchema>;
export type IdentityProfileRequest = z.infer<typeof identityProfileRequestSchema>;
export type RepositoryIdentityBindRequest = z.infer<typeof repositoryIdentityBindRequestSchema>;
export type RepositoryIdentityRequest = z.infer<typeof repositoryIdentityRequestSchema>;
export type RepositoryScanRecord = z.infer<typeof repositoryScanRecordSchema>;
export type ScanProjectResult = z.infer<typeof scanProjectResultSchema>;
export type Overview = z.infer<typeof overviewSchema>;
export type ChangesResult = z.infer<typeof changesResultSchema>;
export type DiffResult = z.infer<typeof diffResultSchema>;
export type WriteResult = z.infer<typeof writeResultSchema>;
export type TrustResponse = z.infer<typeof trustResponseSchema>;
export type TrustStatusResponse = z.infer<typeof trustStatusResponseSchema>;
export type IdentityListResponse = z.infer<typeof identityListResponseSchema>;
export type OperationResponse = z.infer<typeof operationResponseSchema>;
export type RepositoryOperationResponse = z.infer<typeof repositoryOperationResponseSchema>;
export type GitOperationResponse = OperationResponse;
