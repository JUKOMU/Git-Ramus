import { z } from "zod";
import {
  persistedRepositorySnapshotSchema,
  projectSchema,
  repositoryRequestSchema,
  repositorySchema
} from "./git";
import { jobSchema } from "./jobs";
import { remoteRepositorySchema } from "./provider";

const uuid = z.string().uuid();
const timestamp = z.string().datetime({ offset: true });
const nonnegativeInteger = z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER);
const safeName = z.string().trim().min(1).max(128);

const containsControlCharacter = (value: string) =>
  Array.from(value).some((character) => {
    const code = character.charCodeAt(0);
    return code < 0x20 || code === 0x7f;
  });

export const transportRemoteNameSchema = z
  .string()
  .min(1)
  .max(255)
  .refine((value) => !value.startsWith("-"), "remote name looks like an option")
  .refine(
    (value) =>
      !Array.from(value).some((character) => {
        const code = character.charCodeAt(0);
        return code <= 0x20 || code === 0x7f || "~^:?*[\\".includes(character);
      }),
    "unsafe remote name"
  );

export const gitBranchNameSchema = z
  .string()
  .min(1)
  .max(1024)
  .refine((value) => !value.startsWith("-") && !value.startsWith("."), "unsafe branch name")
  .refine(
    (value) =>
      !value.endsWith("/") &&
      !value.endsWith(".") &&
      !value.endsWith(".lock") &&
      !value.includes("..") &&
      !value.includes("@{") &&
      !value.includes("//"),
    "unsafe branch name"
  )
  .refine(
    (value) =>
      !Array.from(value).some((character) => {
        const code = character.charCodeAt(0);
        return code <= 0x20 || code === 0x7f || "~^:?*[\\".includes(character);
      }),
    "unsafe branch name"
  );

const cloneFolderNameSchema = z
  .string()
  .min(1)
  .max(255)
  .refine(
    (value) =>
      value !== "." &&
      value !== ".." &&
      !value.includes("\\") &&
      !value.includes("/") &&
      !containsControlCharacter(value),
    "unsafe clone folder name"
  );

const keyFileNameSchema = z
  .string()
  .min(1)
  .max(255)
  .refine(
    (value) =>
      value !== "." &&
      value !== ".." &&
      !value.includes("\\") &&
      !value.includes("/") &&
      !containsControlCharacter(value),
    "unsafe key filename"
  );

const sanitizedRemoteUrlSchema = z
  .string()
  .min(1)
  .max(4096)
  .refine((value) => !containsControlCharacter(value), "remote URL contains control characters")
  .refine((value) => !/[?#]/u.test(value), "remote URL contains query or fragment data")
  .refine((value) => !/^https:\/\/[^/]*@/iu.test(value), "HTTPS URL contains user info")
  .refine((value) => !/^ssh:\/\/[^/@:]+:[^/@]+@/iu.test(value), "SSH URL contains a password");

export const transportKindSchema = z.enum(["ssh", "https"]);

export const transportProfileSummarySchema = z
  .object({
    id: uuid,
    displayName: safeName,
    kind: transportKindSchema,
    sshKeyFileName: keyFileNameSchema.nullable(),
    sshIdentitiesOnly: z.boolean().nullable(),
    httpsUsername: z.string().trim().min(1).max(256).nullable(),
    available: z.boolean(),
    boundRepositoryCount: nonnegativeInteger
  })
  .strict()
  .superRefine((profile, context) => {
    const valid =
      (profile.kind === "ssh" &&
        profile.sshKeyFileName !== null &&
        profile.sshIdentitiesOnly !== null &&
        profile.httpsUsername === null) ||
      (profile.kind === "https" &&
        profile.sshKeyFileName === null &&
        profile.sshIdentitiesOnly === null &&
        profile.httpsUsername !== null);
    if (!valid) {
      context.addIssue({
        code: "custom",
        message: "transport profile summary fields do not match its kind"
      });
    }
  });

export const transportProfileListResponseSchema = z
  .object({ items: z.array(transportProfileSummarySchema) })
  .strict();

const sshProfileInputShape = {
  kind: z.literal("ssh"),
  displayName: safeName,
  identitiesOnly: z.boolean()
};
const httpsProfileInputShape = {
  kind: z.literal("https"),
  displayName: safeName,
  username: z.string().trim().min(1).max(256),
  useHttpPath: z.literal(true)
};

export const transportProfileCreateRequestSchema = z.discriminatedUnion("kind", [
  z.object({ ...sshProfileInputShape, sshKeyAction: z.literal("selectFile") }).strict(),
  z.object(httpsProfileInputShape).strict()
]);

export const transportProfileUpdateRequestSchema = z.discriminatedUnion("kind", [
  z
    .object({
      profileId: uuid,
      ...sshProfileInputShape,
      sshKeyAction: z.enum(["keep", "selectFile"])
    })
    .strict(),
  z.object({ profileId: uuid, ...httpsProfileInputShape }).strict()
]);

export const transportProfileRequestSchema = z.object({ profileId: uuid }).strict();

export const transportProfileDeletionImpactSchema = z
  .object({
    profileId: uuid,
    repositories: z.array(
      z
        .object({
          repositoryId: uuid,
          displayName: z.string().min(1).max(512),
          transportKind: transportKindSchema
        })
        .strict()
    )
  })
  .strict();

export const transportProfileDeletionResolutionSchema = z.discriminatedUnion("action", [
  z
    .object({
      repositoryId: uuid,
      action: z.literal("replace"),
      replacementProfileId: uuid
    })
    .strict(),
  z
    .object({
      repositoryId: uuid,
      action: z.literal("unbind"),
      driftResolution: z.enum(["reject", "keepExternal"])
    })
    .strict()
]);

export const transportProfileDeleteRequestSchema = z
  .object({
    profileId: uuid,
    resolutions: z.array(transportProfileDeletionResolutionSchema)
  })
  .strict()
  .superRefine((request, context) => {
    const repositoryIds = request.resolutions.map((resolution) => resolution.repositoryId);
    if (new Set(repositoryIds).size !== repositoryIds.length) {
      context.addIssue({ code: "custom", message: "repository resolutions must be unique" });
    }
  });

export const transportDriftStatusSchema = z.enum(["clean", "drifted"]);
export const transportDriftResolutionSchema = z.enum(["reject", "keepExternal", "reapply"]);

export const repositoryTransportBindingSchema = z
  .object({
    repositoryId: uuid,
    transportProfileId: uuid,
    driftStatus: transportDriftStatusSchema,
    boundAt: timestamp,
    updatedAt: timestamp
  })
  .strict();

export const repositoryTransportBindRequestSchema = repositoryRequestSchema.safeExtend({
  transportProfileId: uuid,
  replaceExisting: z.boolean()
});

export const repositoryTransportUnbindRequestSchema = repositoryRequestSchema.safeExtend({
  driftResolution: transportDriftResolutionSchema
});

export const effectiveTransportSchema = z
  .object({
    repositoryId: uuid,
    source: z.enum(["systemGit", "profile"]),
    kind: transportKindSchema.nullable(),
    profile: transportProfileSummarySchema.nullable(),
    driftStatus: transportDriftStatusSchema.nullable()
  })
  .strict()
  .superRefine((transport, context) => {
    const valid =
      (transport.source === "systemGit" &&
        transport.kind === null &&
        transport.profile === null &&
        transport.driftStatus === null) ||
      (transport.source === "profile" &&
        transport.kind !== null &&
        transport.profile !== null &&
        transport.profile.kind === transport.kind &&
        transport.driftStatus !== null);
    if (!valid) {
      context.addIssue({ code: "custom", message: "effective transport fields are inconsistent" });
    }
  });

export const providerCloneIntentCreateRequestSchema = z
  .object({
    accountId: uuid,
    repositoryId: z.string().min(1).max(256)
  })
  .strict();

export const cloneIntentReferenceSchema = z.object({ intentId: uuid }).strict();
export const cloneIntentRequestSchema = cloneIntentReferenceSchema;

export const cloneIntentSummarySchema = z
  .object({
    id: uuid,
    repository: remoteRepositorySchema,
    availableTransports: z.array(transportKindSchema).min(1).max(2),
    createdAt: timestamp,
    expiresAt: timestamp
  })
  .strict()
  .superRefine((intent, context) => {
    if (new Set(intent.availableTransports).size !== intent.availableTransports.length) {
      context.addIssue({ code: "custom", message: "available transports must be unique" });
    }
  });

export const cloneSourceSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("intent"), intentId: uuid }).strict(),
  z
    .object({
      kind: z.literal("manual"),
      remoteUrl: z
        .string()
        .min(1)
        .max(4096)
        .refine((value) => !containsControlCharacter(value))
    })
    .strict()
]);

export const cloneProjectTargetSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("existing"), projectId: uuid }).strict(),
  z.object({ kind: z.literal("new"), name: safeName }).strict()
]);

export const cloneRequestSchema = z
  .object({
    source: cloneSourceSchema,
    transportKind: transportKindSchema,
    profileId: uuid.nullable(),
    folderName: cloneFolderNameSchema,
    projectTarget: cloneProjectTargetSchema,
    operationId: uuid
  })
  .strict();

export const cloneProjectSummarySchema = projectSchema.omit({ rootPath: true });
export const cloneRepositorySummarySchema = repositorySchema.omit({ canonicalPath: true });

export const cloneResultSchema = z
  .object({
    operationId: uuid,
    intentId: uuid.nullable(),
    status: z.enum(["completed", "partial"]),
    job: jobSchema,
    project: cloneProjectSummarySchema,
    repository: cloneRepositorySummarySchema,
    snapshot: persistedRepositorySnapshotSchema.nullable()
  })
  .strict();

export const repositoryRemoteSummarySchema = z
  .object({
    name: transportRemoteNameSchema,
    fetchUrl: sanitizedRemoteUrlSchema,
    pushUrl: sanitizedRemoteUrlSchema.nullable(),
    kind: z.enum(["ssh", "https", "unknown"])
  })
  .strict();

export const upstreamCandidateSchema = z
  .object({
    remoteName: transportRemoteNameSchema,
    branchName: gitBranchNameSchema
  })
  .strict();

export const upstreamCandidateListResponseSchema = z
  .object({ items: z.array(upstreamCandidateSchema) })
  .strict();

export const repositoryOperationInProgressSchema = z.enum([
  "merge",
  "rebase",
  "cherryPick",
  "revert",
  "bisect"
]);

export const repositoryNetworkStateSchema = z
  .object({
    repositoryId: uuid,
    branch: z.string().min(1).max(1024).nullable(),
    detached: z.boolean(),
    upstream: upstreamCandidateSchema.nullable(),
    remotes: z.array(repositoryRemoteSummarySchema),
    ahead: nonnegativeInteger,
    behind: nonnegativeInteger,
    conflictedCount: nonnegativeInteger,
    inProgress: repositoryOperationInProgressSchema.nullable()
  })
  .strict()
  .superRefine((state, context) => {
    if (state.detached !== (state.branch === null)) {
      context.addIssue({ code: "custom", message: "detached state and branch are inconsistent" });
    }
    const remoteNames = state.remotes.map((remote) => remote.name);
    if (new Set(remoteNames).size !== remoteNames.length) {
      context.addIssue({ code: "custom", message: "remote names must be unique" });
    }
  });

const networkRequestBaseSchema = repositoryRequestSchema.safeExtend({ operationId: uuid });

export const repositoryFetchRequestSchema = networkRequestBaseSchema.safeExtend({
  remoteName: transportRemoteNameSchema
});
export const repositoryPullRequestSchema = networkRequestBaseSchema;
export const repositoryPushRequestSchema = networkRequestBaseSchema.safeExtend({
  target: upstreamCandidateSchema.nullable()
});

export const transportOperationCancelRequestSchema = z.object({ operationId: uuid }).strict();

export const networkStageSchema = z.enum([
  "validating",
  "awaitingAuthentication",
  "transferring",
  "checkingOut",
  "applyingProfile",
  "registering",
  "refreshing",
  "completed",
  "failed",
  "cancelled",
  "partial"
]);

export const networkProgressSchema = z
  .object({
    operationId: uuid,
    stage: networkStageSchema,
    fraction: z.number().min(0).max(1).nullable(),
    objects: z
      .object({ completed: nonnegativeInteger, total: nonnegativeInteger.nullable() })
      .strict()
      .nullable(),
    bytes: z
      .object({ transferred: nonnegativeInteger, total: nonnegativeInteger.nullable() })
      .strict()
      .nullable()
  })
  .strict();

export const networkOperationResultSchema = z
  .object({
    operationId: uuid,
    repositoryId: uuid,
    remoteName: transportRemoteNameSchema.nullable(),
    job: jobSchema,
    snapshot: persistedRepositorySnapshotSchema,
    networkState: repositoryNetworkStateSchema
  })
  .strict();

export type TransportKind = z.infer<typeof transportKindSchema>;
export type TransportProfileSummary = z.infer<typeof transportProfileSummarySchema>;
export type TransportProfileListResponse = z.infer<typeof transportProfileListResponseSchema>;
export type TransportProfileCreateRequest = z.infer<typeof transportProfileCreateRequestSchema>;
export type TransportProfileUpdateRequest = z.infer<typeof transportProfileUpdateRequestSchema>;
export type TransportProfileRequest = z.infer<typeof transportProfileRequestSchema>;
export type TransportProfileDeletionImpact = z.infer<typeof transportProfileDeletionImpactSchema>;
export type TransportProfileDeletionResolution = z.infer<
  typeof transportProfileDeletionResolutionSchema
>;
export type TransportProfileDeleteRequest = z.infer<typeof transportProfileDeleteRequestSchema>;
export type TransportDriftStatus = z.infer<typeof transportDriftStatusSchema>;
export type TransportDriftResolution = z.infer<typeof transportDriftResolutionSchema>;
export type RepositoryTransportBinding = z.infer<typeof repositoryTransportBindingSchema>;
export type RepositoryTransportBindRequest = z.infer<typeof repositoryTransportBindRequestSchema>;
export type RepositoryTransportUnbindRequest = z.infer<
  typeof repositoryTransportUnbindRequestSchema
>;
export type EffectiveTransport = z.infer<typeof effectiveTransportSchema>;
export type ProviderCloneIntentCreateRequest = z.infer<
  typeof providerCloneIntentCreateRequestSchema
>;
export type CloneIntentReference = z.infer<typeof cloneIntentReferenceSchema>;
export type CloneIntentRequest = z.infer<typeof cloneIntentRequestSchema>;
export type CloneIntentSummary = z.infer<typeof cloneIntentSummarySchema>;
export type CloneSource = z.infer<typeof cloneSourceSchema>;
export type CloneProjectTarget = z.infer<typeof cloneProjectTargetSchema>;
export type CloneRequest = z.infer<typeof cloneRequestSchema>;
export type CloneProjectSummary = z.infer<typeof cloneProjectSummarySchema>;
export type CloneRepositorySummary = z.infer<typeof cloneRepositorySummarySchema>;
export type CloneResult = z.infer<typeof cloneResultSchema>;
export type RepositoryRemoteSummary = z.infer<typeof repositoryRemoteSummarySchema>;
export type UpstreamCandidate = z.infer<typeof upstreamCandidateSchema>;
export type UpstreamCandidateListResponse = z.infer<typeof upstreamCandidateListResponseSchema>;
export type RepositoryOperationInProgress = z.infer<typeof repositoryOperationInProgressSchema>;
export type RepositoryNetworkState = z.infer<typeof repositoryNetworkStateSchema>;
export type RepositoryFetchRequest = z.infer<typeof repositoryFetchRequestSchema>;
export type RepositoryPullRequest = z.infer<typeof repositoryPullRequestSchema>;
export type RepositoryPushRequest = z.infer<typeof repositoryPushRequestSchema>;
export type TransportOperationCancelRequest = z.infer<typeof transportOperationCancelRequestSchema>;
export type NetworkStage = z.infer<typeof networkStageSchema>;
export type NetworkProgress = z.infer<typeof networkProgressSchema>;
export type NetworkOperationResult = z.infer<typeof networkOperationResultSchema>;
