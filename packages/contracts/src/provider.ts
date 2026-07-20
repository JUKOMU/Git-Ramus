import { z } from "zod";

const uuid = z
  .string()
  .uuid()
  .refine(
    (value) =>
      value === value.toLowerCase() &&
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/u.test(value),
    "UUID must use canonical lowercase hyphenated form"
  );
const nullableUuid = uuid.nullable();
const timestamp = z.string().datetime({ offset: true });
const nonnegativeInteger = z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER);
const httpsUrl = z
  .string()
  .max(4096)
  .url()
  .refine((value) => new URL(value).protocol === "https:", "URL must use HTTPS");
const containsControlCharacter = (value: string) =>
  Array.from(value).some((character) => {
    const code = character.charCodeAt(0);
    return code < 0x20 || code === 0x7f;
  });
const remoteName = z
  .string()
  .min(1)
  .max(256)
  .refine((value) => !containsControlCharacter(value), "remote name contains control characters");

const unique = <T>(values: T[]) => new Set(values).size === values.length;

export const providerKindSchema = z.enum(["github", "gitlab"]);
export const providerConnectionStatusSchema = z.enum([
  "connected",
  "actionRequired",
  "rateLimited",
  "unavailable"
]);
export const providerVisibilitySchema = z.enum(["public", "internal", "private"]);
export const providerPermissionSchema = z.enum(["read", "write", "admin"]);

export const providerInstanceSchema = z
  .object({
    id: uuid,
    providerKind: providerKindSchema,
    displayName: z.string().min(1).max(128),
    baseUrl: httpsUrl,
    customCaConfigured: z.boolean(),
    customCaLabel: z.string().min(1).max(255).nullable(),
    providerEnabled: z.boolean(),
    status: providerConnectionStatusSchema,
    lastValidatedAt: timestamp.nullable(),
    serverVersion: z.string().min(1).max(128).nullable(),
    createdAt: timestamp,
    updatedAt: timestamp
  })
  .strict();

export const providerAccountSummarySchema = z
  .object({
    id: uuid,
    instanceId: uuid,
    providerUserId: z.string().min(1).max(256),
    username: z.string().min(1).max(256),
    displayName: z.string().min(1).max(256).nullable(),
    avatarUrl: z.string().url().nullable(),
    isDefault: z.boolean(),
    status: providerConnectionStatusSchema,
    lastValidatedAt: timestamp
  })
  .strict();

export const remoteRepositorySchema = z
  .object({
    providerKind: providerKindSchema,
    instanceId: uuid,
    repositoryId: z.string().min(1).max(256),
    namespace: z.string().min(1).max(1024),
    name: z.string().min(1).max(512),
    fullName: z.string().min(1).max(1536),
    webUrl: httpsUrl,
    httpsUrl,
    sshUrl: z.string().min(1).max(4096),
    defaultBranch: z.string().min(1).max(1024).nullable(),
    visibility: providerVisibilitySchema,
    archived: z.boolean(),
    fork: z.boolean(),
    permission: providerPermissionSchema,
    updatedAt: timestamp
  })
  .strict();

const providerInstanceModeSchema = z.enum(["cloud", "selfHosted"]);
const providerCapabilitySchema = z.enum(["repositoryDiscovery", "customCa"]);

export const providerContributionSchema = z
  .object({
    providerId: providerKindSchema,
    adapterId: z.string().regex(/^[a-z0-9]+(?:[.-][a-z0-9]+)+$/u),
    displayName: z.string().min(1).max(64),
    icon: z.enum(["github", "gitlab"]),
    instanceModes: z.array(providerInstanceModeSchema).min(1),
    capabilities: z.array(providerCapabilitySchema).min(1)
  })
  .strict()
  .superRefine((contribution, context) => {
    if (!unique(contribution.instanceModes)) {
      context.addIssue({ code: "custom", message: "Provider instance modes must be unique" });
    }
    if (!unique(contribution.capabilities)) {
      context.addIssue({ code: "custom", message: "Provider capabilities must be unique" });
    }
    if (!contribution.capabilities.includes("repositoryDiscovery")) {
      context.addIssue({ code: "custom", message: "Provider must support repository discovery" });
    }
    if (contribution.icon !== contribution.providerId) {
      context.addIssue({ code: "custom", message: "Provider icon must match its Provider kind" });
    }
    if (
      contribution.providerId === "github" &&
      (contribution.instanceModes.length !== 1 ||
        contribution.instanceModes[0] !== "cloud" ||
        contribution.capabilities.length !== 1 ||
        contribution.capabilities[0] !== "repositoryDiscovery")
    ) {
      context.addIssue({
        code: "custom",
        message: "GitHub supports cloud repository discovery only"
      });
    }
  });

export const providerInstanceCreateRequestSchema = z
  .object({
    providerKind: providerKindSchema,
    displayName: z.string().trim().min(1).max(128),
    baseUrl: httpsUrl,
    customCaAction: z.enum(["none", "selectFile"])
  })
  .strict()
  .refine(
    (value) => value.providerKind === "gitlab" || value.customCaAction === "none",
    "GitHub does not support custom CA files"
  );

export const providerInstanceUpdateRequestSchema = z
  .object({
    instanceId: uuid,
    displayName: z.string().trim().min(1).max(128),
    baseUrl: httpsUrl,
    customCaAction: z.enum(["keep", "remove", "selectFile"])
  })
  .strict()
  .refine(
    (value) =>
      value.customCaAction !== "selectFile" ||
      !/^https:\/\/github\.com(?:\/|$)/u.test(value.baseUrl),
    "GitHub does not support custom CA files"
  );

export const providerInstanceRequestSchema = z.object({ instanceId: uuid }).strict();
export const providerInstanceListRequestSchema = z.object({}).strict();
export const providerInstanceListResponseSchema = z
  .object({ items: z.array(providerInstanceSchema) })
  .strict();

export const providerAccountListRequestSchema = z.object({ instanceId: uuid }).strict();
export const providerAccountListResponseSchema = z
  .object({ items: z.array(providerAccountSummarySchema) })
  .strict();
export const providerAccountConnectRequestSchema = z.object({ instanceId: uuid }).strict();
export const providerAccountRotateRequestSchema = z.object({ accountId: uuid }).strict();
export const providerAccountValidateRequestSchema = z.object({ accountId: uuid }).strict();
export const providerAccountSetDefaultRequestSchema = z
  .object({ instanceId: uuid, accountId: uuid })
  .strict();
export const providerAccountDeletionImpactRequestSchema = z.object({ accountId: uuid }).strict();

export const providerAccountDeletionResolutionSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("reassign"), accountId: uuid }).strict(),
  z.object({ kind: z.literal("inherit") }).strict(),
  z.object({ kind: z.literal("unbind") }).strict()
]);

export const providerAccountDeleteRequestSchema = z
  .object({
    accountId: uuid,
    resolution: providerAccountDeletionResolutionSchema,
    newDefaultAccountId: nullableUuid.default(null)
  })
  .strict();

export const providerRepositoryQuerySchema = z
  .object({
    search: z.string().trim().max(256),
    visibility: providerVisibilitySchema.nullable(),
    namespace: z.string().trim().min(1).max(1024).nullable(),
    archived: z.enum(["all", "active", "archived"]),
    sort: z.enum(["name", "updated"]),
    direction: z.enum(["asc", "desc"]),
    pageSize: z.number().int().min(1).max(100)
  })
  .strict();

export const providerRepositoryListRequestSchema = z
  .object({
    accountId: uuid,
    query: providerRepositoryQuerySchema,
    cursor: nullableUuid,
    operationId: uuid
  })
  .strict();

export const providerOperationCancelRequestSchema = z
  .object({ accountId: uuid, operationId: uuid })
  .strict();

export const providerRateLimitStateSchema = z
  .object({
    limit: nonnegativeInteger.nullable(),
    remaining: nonnegativeInteger.nullable(),
    resetAt: timestamp.nullable(),
    retryAfterMs: nonnegativeInteger.nullable()
  })
  .strict();

export const providerRepositoryPageSchema = z
  .object({
    items: z.array(remoteRepositorySchema),
    nextCursor: nullableUuid,
    hasMore: z.boolean(),
    rateLimit: providerRateLimitStateSchema.nullable()
  })
  .strict();

export const providerBindingSchema = z
  .object({
    repositoryId: uuid,
    remoteName,
    providerInstanceId: uuid,
    providerAccountId: nullableUuid,
    providerRepositoryId: z.string().min(1).max(256),
    fullName: z.string().min(1).max(1536),
    webUrl: httpsUrl,
    matchedUrl: z.string().min(1).max(4096),
    bindingSource: z.enum(["auto", "manual"]),
    boundAt: timestamp,
    updatedAt: timestamp
  })
  .strict();

export const providerBindingSuggestionSchema = z
  .object({
    repositoryId: uuid,
    remoteName,
    instanceId: uuid,
    status: z.enum(["suggested", "ambiguous", "unverified", "none"]),
    providerRepositoryId: z.string().min(1).max(256).nullable(),
    fullName: z.string().min(1).max(1536).nullable(),
    webUrl: httpsUrl.nullable(),
    matchedUrl: z.string().min(1).max(4096).nullable(),
    candidates: z.array(remoteRepositorySchema).max(32)
  })
  .strict();

export const providerAccountDeletionImpactSchema = z
  .object({
    accountId: uuid,
    instanceId: uuid,
    isDefault: z.boolean(),
    explicitBindingCount: nonnegativeInteger,
    inheritedBindingCount: nonnegativeInteger,
    siblingAccountIds: z.array(uuid),
    requiresNewDefault: z.boolean()
  })
  .strict();

export const providerAuthorizedAccountSchema = z
  .object({
    instance: providerInstanceSchema,
    account: providerAccountSummarySchema
  })
  .strict();

export const providerAuthorizedAccountListResponseSchema = z
  .object({ items: z.array(providerAuthorizedAccountSchema) })
  .strict();
export const providerReadAccessRequestSchema = z.object({}).strict();
export const providerReadAccessRevokeRequestSchema = z.object({ accountId: uuid }).strict();

export const providerLocalRemoteMatchRequestSchema = z
  .object({ instanceId: uuid, accountId: uuid, operationId: uuid })
  .strict();
export const providerBindingSuggestionListResponseSchema = z
  .object({ items: z.array(providerBindingSuggestionSchema) })
  .strict();
export const providerBindingListRequestSchema = z.object({ accountId: uuid }).strict();
export const providerBindingListResponseSchema = z
  .object({ items: z.array(providerBindingSchema) })
  .strict();
export const providerBindingSetRequestSchema = z
  .object({
    repositoryId: uuid,
    remoteName,
    instanceId: uuid,
    accountId: nullableUuid,
    providerRepositoryId: z.string().min(1).max(256)
  })
  .strict();
export const providerBindingDeleteRequestSchema = z
  .object({ repositoryId: uuid, remoteName })
  .strict();

export type ProviderKind = z.infer<typeof providerKindSchema>;
export type ProviderConnectionStatus = z.infer<typeof providerConnectionStatusSchema>;
export type ProviderVisibility = z.infer<typeof providerVisibilitySchema>;
export type ProviderPermission = z.infer<typeof providerPermissionSchema>;
export type ProviderInstance = z.infer<typeof providerInstanceSchema>;
export type ProviderAccountSummary = z.infer<typeof providerAccountSummarySchema>;
export type RemoteRepository = z.infer<typeof remoteRepositorySchema>;
export type ProviderContribution = z.infer<typeof providerContributionSchema>;
export type ProviderInstanceCreateRequest = z.infer<typeof providerInstanceCreateRequestSchema>;
export type ProviderInstanceUpdateRequest = z.infer<typeof providerInstanceUpdateRequestSchema>;
export type ProviderInstanceRequest = z.infer<typeof providerInstanceRequestSchema>;
export type ProviderInstanceListResponse = z.infer<typeof providerInstanceListResponseSchema>;
export type ProviderAccountListRequest = z.infer<typeof providerAccountListRequestSchema>;
export type ProviderAccountListResponse = z.infer<typeof providerAccountListResponseSchema>;
export type ProviderAccountConnectRequest = z.infer<typeof providerAccountConnectRequestSchema>;
export type ProviderAccountRotateRequest = z.infer<typeof providerAccountRotateRequestSchema>;
export type ProviderAccountValidateRequest = z.infer<typeof providerAccountValidateRequestSchema>;
export type ProviderAccountSetDefaultRequest = z.infer<
  typeof providerAccountSetDefaultRequestSchema
>;
export type ProviderAccountDeletionImpactRequest = z.infer<
  typeof providerAccountDeletionImpactRequestSchema
>;
export type ProviderAccountDeletionResolution = z.infer<
  typeof providerAccountDeletionResolutionSchema
>;
export type ProviderAccountDeleteRequest = z.infer<typeof providerAccountDeleteRequestSchema>;
export type ProviderRepositoryQuery = z.infer<typeof providerRepositoryQuerySchema>;
export type ProviderRepositoryListRequest = z.infer<typeof providerRepositoryListRequestSchema>;
export type ProviderOperationCancelRequest = z.infer<typeof providerOperationCancelRequestSchema>;
export type ProviderRateLimitState = z.infer<typeof providerRateLimitStateSchema>;
export type ProviderRepositoryPage = z.infer<typeof providerRepositoryPageSchema>;
export type ProviderBinding = z.infer<typeof providerBindingSchema>;
export type ProviderBindingSuggestion = z.infer<typeof providerBindingSuggestionSchema>;
export type ProviderAccountDeletionImpact = z.infer<typeof providerAccountDeletionImpactSchema>;
export type ProviderAuthorizedAccount = z.infer<typeof providerAuthorizedAccountSchema>;
export type ProviderAuthorizedAccountListResponse = z.infer<
  typeof providerAuthorizedAccountListResponseSchema
>;
export type ProviderReadAccessRevokeRequest = z.infer<typeof providerReadAccessRevokeRequestSchema>;
export type ProviderLocalRemoteMatchRequest = z.infer<typeof providerLocalRemoteMatchRequestSchema>;
export type ProviderBindingListRequest = z.infer<typeof providerBindingListRequestSchema>;
export type ProviderBindingListResponse = z.infer<typeof providerBindingListResponseSchema>;
export type ProviderBindingSetRequest = z.infer<typeof providerBindingSetRequestSchema>;
export type ProviderBindingDeleteRequest = z.infer<typeof providerBindingDeleteRequestSchema>;
