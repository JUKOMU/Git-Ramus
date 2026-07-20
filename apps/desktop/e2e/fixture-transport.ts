import { invokeHost } from "./fixture-project";

export interface TransportFixture {
  projectId: string;
  projectName: "E2E Transport";
  repositoryName: "private-skill";
  branchName: "main";
  remoteName: "origin";
  cleanupToken: string;
}

export interface TransportBlockStatus {
  connected: boolean;
  active: boolean;
}

export async function seedTransportFixture(): Promise<TransportFixture> {
  return parseTransportFixture(await invokeHost("e2e_seed_transport_fixture", {}));
}

export async function advanceTransportRemote(fixture: TransportFixture): Promise<string> {
  return parseHead(await invokeHost("e2e_advance_transport_remote", tokenRequest(fixture)));
}

export async function commitTransportLocal(
  fixture: TransportFixture,
  repositoryId: string
): Promise<string> {
  return parseHead(
    await invokeHost("e2e_commit_transport_local", {
      request: {
        ...tokenRequest(fixture).request,
        projectId: uuid(fixture.projectId),
        repositoryId: uuid(repositoryId)
      }
    })
  );
}

export async function transportRemoteHead(fixture: TransportFixture): Promise<string> {
  return parseHead(await invokeHost("e2e_transport_remote_head", tokenRequest(fixture)));
}

export async function blockNextTransportFetch(fixture: TransportFixture): Promise<void> {
  await invokeHost("e2e_block_transport_fetch", tokenRequest(fixture));
}

export async function transportBlockStatus(
  fixture: TransportFixture
): Promise<TransportBlockStatus> {
  const status = strictRecord(
    await invokeHost("e2e_transport_block_status", tokenRequest(fixture)),
    ["connected", "active"]
  );
  if (typeof status.connected !== "boolean" || typeof status.active !== "boolean") {
    throw new Error("Transport E2E block status is invalid");
  }
  return { connected: status.connected, active: status.active };
}

export async function cleanupTransportFixture(fixture: TransportFixture): Promise<void> {
  await invokeHost("e2e_cleanup_transport_fixture", tokenRequest(fixture));
}

function tokenRequest(fixture: TransportFixture): { request: { cleanupToken: string } } {
  return { request: { cleanupToken: uuid(fixture.cleanupToken) } };
}

function parseTransportFixture(value: unknown): TransportFixture {
  const fixture = strictRecord(value, [
    "projectId",
    "projectName",
    "repositoryName",
    "branchName",
    "remoteName",
    "cleanupToken"
  ]);
  return {
    projectId: uuid(fixture.projectId),
    projectName: literal(fixture.projectName, "E2E Transport"),
    repositoryName: literal(fixture.repositoryName, "private-skill"),
    branchName: literal(fixture.branchName, "main"),
    remoteName: literal(fixture.remoteName, "origin"),
    cleanupToken: uuid(fixture.cleanupToken)
  };
}

function parseHead(value: unknown): string {
  const response = strictRecord(value, ["headOid"]);
  if (
    typeof response.headOid !== "string" ||
    !/^(?:[0-9a-f]{40}|[0-9a-f]{64})$/u.test(response.headOid)
  ) {
    throw new Error("Transport E2E Git OID is invalid");
  }
  return response.headOid;
}

function strictRecord(value: unknown, keys: string[]): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("Transport E2E response is not an object");
  }
  const record = value as Record<string, unknown>;
  const actual = Object.keys(record).sort();
  const expected = [...keys].sort();
  if (actual.length !== expected.length || actual.some((key, index) => key !== expected[index])) {
    throw new Error("Transport E2E response has unexpected fields");
  }
  return record;
}

function uuid(value: unknown): string {
  if (
    typeof value !== "string" ||
    !/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u.test(value)
  ) {
    throw new Error("Transport E2E ID is not a canonical UUID v4");
  }
  return value;
}

function literal<T extends string>(value: unknown, expected: T): T {
  if (value !== expected) throw new Error("Transport E2E literal is invalid");
  return expected;
}
