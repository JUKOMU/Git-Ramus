import { invokeHost, invokeHostResult } from "./fixture-project";

export interface ProviderFixture {
  instance: ProviderInstanceSummary;
  account: ProviderAccountSummary;
  repository: ProviderRepositorySummary;
}

interface ProviderInstanceSummary {
  id: string;
  providerKind: "gitlab";
  displayName: string;
  baseUrl: string;
  customCaConfigured: boolean;
  customCaLabel: string | null;
  providerEnabled: boolean;
  status: "connected" | "actionRequired" | "rateLimited" | "unavailable";
  lastValidatedAt: string | null;
  serverVersion: string | null;
  createdAt: string;
  updatedAt: string;
}

interface ProviderAccountSummary {
  id: string;
  instanceId: string;
  providerUserId: string;
  username: string;
  displayName: string | null;
  avatarUrl: string | null;
  isDefault: boolean;
  status: "connected" | "actionRequired" | "rateLimited" | "unavailable";
  lastValidatedAt: string;
}

interface ProviderRepositorySummary {
  providerKind: "gitlab";
  instanceId: string;
  repositoryId: string;
  namespace: string;
  name: string;
  fullName: string;
  webUrl: string;
  httpsUrl: string;
  sshUrl: string;
  defaultBranch: string | null;
  visibility: "public" | "internal" | "private";
  archived: boolean;
  fork: boolean;
  permission: "read" | "write" | "admin";
  updatedAt: string;
}

export async function seedProviderFixture(): Promise<ProviderFixture> {
  return parseProviderFixture(await invokeHost("e2e_seed_provider_fixture", {}));
}

export async function cleanupProviderFixture(fixture: ProviderFixture): Promise<void> {
  const errors: unknown[] = [];
  await collectDeleteFailure(errors, "provider_account_delete", {
    request: {
      accountId: fixture.account.id,
      resolution: { kind: "unbind" },
      newDefaultAccountId: null
    }
  });
  await collectDeleteFailure(errors, "provider_instance_delete", {
    request: { instanceId: fixture.instance.id }
  });
  if (errors.length > 0) {
    throw new AggregateError(errors, "Provider E2E fixture cleanup failed");
  }
}

function parseProviderFixture(value: unknown): ProviderFixture {
  const fixture = strictRecord(value, ["instance", "account", "repository"]);
  const instance = parseInstance(fixture.instance);
  const account = parseAccount(fixture.account);
  const repository = parseRepository(fixture.repository);
  if (account.instanceId !== instance.id || repository.instanceId !== instance.id) {
    throw new Error("Provider E2E fixture references the wrong instance");
  }
  if (repository.repositoryId !== "4242" || repository.fullName !== "skills/private-skill") {
    throw new Error("Provider E2E fixture repository is not deterministic");
  }
  if (repository.sshUrl !== "git@gitlab.example.test:skills/private-skill.git") {
    throw new Error("Provider E2E fixture SSH URL is unexpected");
  }
  return { instance, account, repository };
}

function parseInstance(value: unknown): ProviderInstanceSummary {
  const item = strictRecord(value, [
    "id",
    "providerKind",
    "displayName",
    "baseUrl",
    "customCaConfigured",
    "customCaLabel",
    "providerEnabled",
    "status",
    "lastValidatedAt",
    "serverVersion",
    "createdAt",
    "updatedAt"
  ]);
  const instance = {
    id: uuid(item.id),
    providerKind: literal(item.providerKind, "gitlab"),
    displayName: nonEmptyString(item.displayName),
    baseUrl: httpsUrl(item.baseUrl),
    customCaConfigured: boolean(item.customCaConfigured),
    customCaLabel: nullableString(item.customCaLabel),
    providerEnabled: boolean(item.providerEnabled),
    status: status(item.status),
    lastValidatedAt: nullableTimestamp(item.lastValidatedAt),
    serverVersion: nullableString(item.serverVersion),
    createdAt: timestamp(item.createdAt),
    updatedAt: timestamp(item.updatedAt)
  } satisfies ProviderInstanceSummary;
  if (instance.baseUrl !== "https://gitlab.example.test") {
    throw new Error("Provider E2E fixture instance URL is unexpected");
  }
  return instance;
}

function parseAccount(value: unknown): ProviderAccountSummary {
  const item = strictRecord(value, [
    "id",
    "instanceId",
    "providerUserId",
    "username",
    "displayName",
    "avatarUrl",
    "isDefault",
    "status",
    "lastValidatedAt"
  ]);
  return {
    id: uuid(item.id),
    instanceId: uuid(item.instanceId),
    providerUserId: nonEmptyString(item.providerUserId),
    username: nonEmptyString(item.username),
    displayName: nullableString(item.displayName),
    avatarUrl: nullableString(item.avatarUrl),
    isDefault: boolean(item.isDefault),
    status: status(item.status),
    lastValidatedAt: timestamp(item.lastValidatedAt)
  };
}

function parseRepository(value: unknown): ProviderRepositorySummary {
  const item = strictRecord(value, [
    "providerKind",
    "instanceId",
    "repositoryId",
    "namespace",
    "name",
    "fullName",
    "webUrl",
    "httpsUrl",
    "sshUrl",
    "defaultBranch",
    "visibility",
    "archived",
    "fork",
    "permission",
    "updatedAt"
  ]);
  return {
    providerKind: literal(item.providerKind, "gitlab"),
    instanceId: uuid(item.instanceId),
    repositoryId: nonEmptyString(item.repositoryId),
    namespace: nonEmptyString(item.namespace),
    name: nonEmptyString(item.name),
    fullName: nonEmptyString(item.fullName),
    webUrl: httpsUrl(item.webUrl),
    httpsUrl: httpsUrl(item.httpsUrl),
    sshUrl: nonEmptyString(item.sshUrl),
    defaultBranch: nullableString(item.defaultBranch),
    visibility: literalUnion(item.visibility, ["public", "internal", "private"]),
    archived: boolean(item.archived),
    fork: boolean(item.fork),
    permission: literalUnion(item.permission, ["read", "write", "admin"]),
    updatedAt: timestamp(item.updatedAt)
  };
}

async function collectDeleteFailure(
  errors: unknown[],
  command: string,
  args: unknown
): Promise<void> {
  try {
    const result = await invokeHostResult(command, args);
    if (!result.ok && recordCode(result.error) !== "resource.not-found") errors.push(result.error);
  } catch (error: unknown) {
    errors.push(error);
  }
}

function strictRecord(value: unknown, keys: string[]): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("Provider E2E response is not an object");
  }
  const record = value as Record<string, unknown>;
  const actual = Object.keys(record).sort();
  const expected = [...keys].sort();
  if (actual.length !== expected.length || actual.some((key, index) => key !== expected[index])) {
    throw new Error("Provider E2E response has unexpected fields");
  }
  return record;
}

function uuid(value: unknown): string {
  const text = nonEmptyString(value);
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u.test(text)) {
    throw new Error("Provider E2E ID is not a UUID v4");
  }
  return text;
}

function httpsUrl(value: unknown): string {
  const text = nonEmptyString(value);
  const parsed = new URL(text);
  if (
    parsed.protocol !== "https:" ||
    parsed.username ||
    parsed.password ||
    parsed.search ||
    parsed.hash
  ) {
    throw new Error("Provider E2E URL is not a clean HTTPS URL");
  }
  return text;
}

function timestamp(value: unknown): string {
  const text = nonEmptyString(value);
  if (Number.isNaN(Date.parse(text))) throw new Error("Provider E2E timestamp is invalid");
  return text;
}

function nullableTimestamp(value: unknown): string | null {
  return value === null ? null : timestamp(value);
}

function nullableString(value: unknown): string | null {
  if (value === null) return null;
  return nonEmptyString(value);
}

function nonEmptyString(value: unknown): string {
  if (typeof value !== "string" || value.length === 0)
    throw new Error("Provider E2E string is empty");
  return value;
}

function boolean(value: unknown): boolean {
  if (typeof value !== "boolean") throw new Error("Provider E2E boolean is invalid");
  return value;
}

function literal<T extends string>(value: unknown, expected: T): T {
  if (value !== expected) throw new Error("Provider E2E literal is invalid");
  return expected;
}

function literalUnion<T extends string>(value: unknown, allowed: readonly T[]): T {
  if (typeof value !== "string" || !allowed.includes(value as T)) {
    throw new Error("Provider E2E enum is invalid");
  }
  return value as T;
}

function status(value: unknown): ProviderInstanceSummary["status"] {
  return literalUnion(value, ["connected", "actionRequired", "rateLimited", "unavailable"]);
}

function recordCode(value: unknown): string | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const code = (value as Record<string, unknown>).code;
  return typeof code === "string" ? code : null;
}
