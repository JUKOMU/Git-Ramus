import {
  errorEnvelopeSchema,
  providerAccountConnectRequestSchema,
  providerAccountDeleteRequestSchema,
  providerAccountDeletionImpactRequestSchema,
  providerAccountDeletionImpactSchema,
  providerAccountListRequestSchema,
  providerAccountListResponseSchema,
  providerAccountRotateRequestSchema,
  providerAccountSetDefaultRequestSchema,
  providerAccountSummarySchema,
  providerAccountValidateRequestSchema,
  providerAuthorizedAccountListResponseSchema,
  providerBindingDeleteRequestSchema,
  providerBindingListRequestSchema,
  providerBindingListResponseSchema,
  providerBindingSchema,
  providerBindingSetRequestSchema,
  providerBindingSuggestionListResponseSchema,
  providerInstanceCreateRequestSchema,
  providerInstanceListResponseSchema,
  providerInstanceRequestSchema,
  providerInstanceSchema,
  providerInstanceUpdateRequestSchema,
  providerLocalRemoteMatchRequestSchema,
  providerOperationCancelRequestSchema,
  providerReadAccessRevokeRequestSchema,
  providerRepositoryListRequestSchema,
  providerRepositoryPageSchema,
  type ProviderAccountDeleteRequest,
  type ProviderAccountDeletionImpact,
  type ProviderAccountListResponse,
  type ProviderAccountSummary,
  type ProviderAuthorizedAccountListResponse,
  type ProviderBinding,
  type ProviderBindingListResponse,
  type ProviderBindingSetRequest,
  type ProviderBindingSuggestion,
  type ProviderInstance,
  type ProviderInstanceCreateRequest,
  type ProviderInstanceListResponse,
  type ProviderInstanceRequest,
  type ProviderInstanceUpdateRequest,
  type ProviderLocalRemoteMatchRequest,
  type ProviderOperationCancelRequest,
  type ProviderRepositoryListRequest,
  type ProviderRepositoryPage,
  type ErrorEnvelope
} from "@git-ramus/contracts";
import { createRequestId, type PluginClient } from "@git-ramus/plugin-sdk";

export interface ProviderCenterApi {
  listInstances(): Promise<ProviderInstanceListResponse>;
  createInstance(request: ProviderInstanceCreateRequest): Promise<ProviderInstance | null>;
  updateInstance(request: ProviderInstanceUpdateRequest): Promise<ProviderInstance | null>;
  validateInstance(request: ProviderInstanceRequest): Promise<ProviderInstance>;
  deleteInstance(request: ProviderInstanceRequest): Promise<void>;
  listAccounts(instanceId: string): Promise<ProviderAccountListResponse>;
  connectAccount(instanceId: string): Promise<ProviderAccountSummary | null>;
  rotateAccount(accountId: string): Promise<ProviderAccountSummary | null>;
  validateAccount(accountId: string): Promise<ProviderAccountSummary>;
  setDefaultAccount(instanceId: string, accountId: string): Promise<ProviderAccountSummary>;
  getAccountDeletionImpact(accountId: string): Promise<ProviderAccountDeletionImpact>;
  deleteAccount(request: ProviderAccountDeleteRequest): Promise<void>;
  listAuthorizedAccounts(): Promise<ProviderAuthorizedAccountListResponse>;
  requestReadAccess(): Promise<ProviderAuthorizedAccountListResponse | null>;
  revokeReadAccess(accountId: string): Promise<void>;
  listRepositories(
    input: Omit<ProviderRepositoryListRequest, "operationId">,
    signal?: AbortSignal
  ): Promise<ProviderRepositoryPage>;
  cancelOperation(request: ProviderOperationCancelRequest): Promise<void>;
  matchLocalRemotes(
    input: Omit<ProviderLocalRemoteMatchRequest, "operationId">,
    signal?: AbortSignal
  ): Promise<{ items: ProviderBindingSuggestion[] }>;
  listBindings(accountId: string): Promise<ProviderBindingListResponse>;
  bindRemote(request: ProviderBindingSetRequest): Promise<ProviderBinding>;
  unbindRemote(repositoryId: string, remoteName: string): Promise<void>;
}

export function createProviderCenterApi(client: PluginClient): ProviderCenterApi {
  return {
    listInstances: () =>
      requestParsed(
        client,
        "providers.listInstances",
        {},
        providerInstanceListResponseSchema,
        "Unable to load Provider instances"
      ),
    createInstance: (request) =>
      requestParsed(
        client,
        "providers.createInstance",
        providerInstanceCreateRequestSchema.parse(request),
        providerInstanceSchema.nullable(),
        "Unable to create the Provider instance"
      ),
    updateInstance: (request) =>
      requestParsed(
        client,
        "providers.updateInstance",
        providerInstanceUpdateRequestSchema.parse(request),
        providerInstanceSchema.nullable(),
        "Unable to update the Provider instance"
      ),
    validateInstance: (request) =>
      requestParsed(
        client,
        "providers.validateInstance",
        providerInstanceRequestSchema.parse(request),
        providerInstanceSchema,
        "Unable to validate the Provider instance"
      ),
    deleteInstance: (request) =>
      requestVoid(
        client,
        "providers.deleteInstance",
        providerInstanceRequestSchema.parse(request),
        "Unable to delete the Provider instance"
      ),
    listAccounts: (instanceId) =>
      requestParsed(
        client,
        "providers.listAccounts",
        providerAccountListRequestSchema.parse({ instanceId }),
        providerAccountListResponseSchema,
        "Unable to load Provider accounts"
      ),
    connectAccount: (instanceId) =>
      requestParsed(
        client,
        "providers.connectAccount",
        providerAccountConnectRequestSchema.parse({ instanceId }),
        providerAccountSummarySchema.nullable(),
        "Unable to connect the Provider account"
      ),
    rotateAccount: (accountId) =>
      requestParsed(
        client,
        "providers.rotateAccount",
        providerAccountRotateRequestSchema.parse({ accountId }),
        providerAccountSummarySchema.nullable(),
        "Unable to rotate the Provider account"
      ),
    validateAccount: (accountId) =>
      requestParsed(
        client,
        "providers.validateAccount",
        providerAccountValidateRequestSchema.parse({ accountId }),
        providerAccountSummarySchema,
        "Unable to validate the Provider account"
      ),
    setDefaultAccount: (instanceId, accountId) =>
      requestParsed(
        client,
        "providers.setDefaultAccount",
        providerAccountSetDefaultRequestSchema.parse({ instanceId, accountId }),
        providerAccountSummarySchema,
        "Unable to change the default Provider account"
      ),
    getAccountDeletionImpact: (accountId) =>
      requestParsed(
        client,
        "providers.getAccountDeletionImpact",
        providerAccountDeletionImpactRequestSchema.parse({ accountId }),
        providerAccountDeletionImpactSchema,
        "Unable to inspect Provider account deletion"
      ),
    deleteAccount: (request) =>
      requestVoid(
        client,
        "providers.deleteAccount",
        providerAccountDeleteRequestSchema.parse(request),
        "Unable to delete the Provider account"
      ),
    listAuthorizedAccounts: () =>
      requestParsed(
        client,
        "providers.listAuthorizedAccounts",
        {},
        providerAuthorizedAccountListResponseSchema,
        "Unable to load authorized Provider accounts"
      ),
    requestReadAccess: () =>
      requestParsed(
        client,
        "providers.requestReadAccess",
        {},
        providerAuthorizedAccountListResponseSchema.nullable(),
        "Unable to request Provider access"
      ),
    revokeReadAccess: (accountId) =>
      requestVoid(
        client,
        "providers.revokeReadAccess",
        providerReadAccessRevokeRequestSchema.parse({ accountId }),
        "Unable to revoke Provider access"
      ),
    listRepositories: (input, signal) => {
      const parsed = providerRepositoryListRequestSchema.omit({ operationId: true }).parse(input);
      return cancellableRequest(
        client,
        "providers.listRepositories",
        parsed,
        providerRepositoryListRequestSchema,
        providerRepositoryPageSchema,
        signal,
        "Unable to load Provider repositories"
      );
    },
    cancelOperation: (request) =>
      requestVoid(
        client,
        "providers.cancelOperation",
        providerOperationCancelRequestSchema.parse(request),
        "Unable to cancel the Provider request"
      ),
    matchLocalRemotes: (input, signal) => {
      const parsed = providerLocalRemoteMatchRequestSchema.omit({ operationId: true }).parse(input);
      return cancellableRequest(
        client,
        "providers.matchLocalRemotes",
        parsed,
        providerLocalRemoteMatchRequestSchema,
        providerBindingSuggestionListResponseSchema,
        signal,
        "Unable to match local remotes"
      );
    },
    listBindings: (accountId) =>
      requestParsed(
        client,
        "providers.listBindings",
        providerBindingListRequestSchema.parse({ accountId }),
        providerBindingListResponseSchema,
        "Unable to load remote bindings"
      ),
    bindRemote: (request) =>
      requestParsed(
        client,
        "providers.bindRemote",
        providerBindingSetRequestSchema.parse(request),
        providerBindingSchema,
        "Unable to bind the local remote"
      ),
    unbindRemote: (repositoryId, remoteName) =>
      requestVoid(
        client,
        "providers.unbindRemote",
        providerBindingDeleteRequestSchema.parse({ repositoryId, remoteName }),
        "Unable to remove the remote binding"
      )
  };
}

async function requestParsed<T>(
  client: PluginClient,
  method: string,
  params: unknown,
  schema: { parse(value: unknown): T },
  fallbackMessage: string
): Promise<T> {
  try {
    return schema.parse(await client.request<unknown>(method, params));
  } catch (error) {
    throw normalizeError(error, fallbackMessage);
  }
}

async function requestVoid(
  client: PluginClient,
  method: string,
  params: unknown,
  fallbackMessage: string
): Promise<void> {
  try {
    await client.request<unknown>(method, params);
  } catch (error) {
    throw normalizeError(error, fallbackMessage);
  }
}

async function cancellableRequest<T>(
  client: PluginClient,
  method: string,
  input: Record<string, unknown>,
  requestSchema: {
    parse(value: unknown): Record<string, unknown> & { accountId: string; operationId: string };
  },
  schema: { parse(value: unknown): T },
  signal: AbortSignal | undefined,
  fallbackMessage: string
): Promise<T> {
  if (signal?.aborted) throw abortError();
  const operationId = operationIdForRequest();
  const requestInput = requestSchema.parse({ ...input, operationId });
  const request = client.request<unknown>(method, requestInput);
  let onAbort: (() => void) | undefined;
  const aborted = new Promise<never>((_, reject) => {
    onAbort = () => {
      void client
        .request("providers.cancelOperation", { accountId: requestInput.accountId, operationId })
        .catch(() => undefined);
      reject(abortError());
    };
    signal?.addEventListener("abort", onAbort, { once: true });
    if (signal?.aborted) onAbort();
  });
  request.catch(() => undefined);
  try {
    const value = signal === undefined ? await request : await Promise.race([request, aborted]);
    return schema.parse(value);
  } catch (error) {
    if (isAbortError(error)) throw error;
    throw normalizeError(error, fallbackMessage);
  } finally {
    if (onAbort !== undefined) signal?.removeEventListener("abort", onAbort);
  }
}

function operationIdForRequest(): string {
  return createRequestId();
}

function abortError(): DOMException {
  return new DOMException("The operation was aborted", "AbortError");
}

function isAbortError(error: unknown): error is DOMException {
  return error instanceof DOMException && error.name === "AbortError";
}

export function normalizeError(error: unknown, fallbackMessage: string): ErrorEnvelope {
  const parsed = errorEnvelopeSchema.safeParse(error);
  if (parsed.success) return parsed.data;
  return {
    code: "provider.unexpected-error",
    category: "retryable",
    message: fallbackMessage,
    operationId: null,
    pluginId: "git-ramus.provider-center",
    resourceId: null,
    failedStep: null,
    retryable: true,
    retryAfterMs: null,
    recoveryActions: [{ id: "retry-provider", label: "Try again", kind: "retry" }],
    details: null
  };
}
