import type { ProviderAccountSummary, ProviderInstance } from "@git-ramus/contracts";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ProviderCenterApi } from "./api";
import { normalizeError } from "./api";
import { AccountPanel } from "./components/AccountPanel";
import { ErrorNotice, InstancePanel } from "./components/InstancePanel";
import { RemoteBindings } from "./components/RemoteBindings";
import { RepositoryBrowser } from "./components/RepositoryBrowser";

interface AppProps {
  api: ProviderCenterApi;
  route: string;
}

export function App({ api, route }: AppProps) {
  const [instances, setInstances] = useState<ProviderInstance[]>([]);
  const [accounts, setAccounts] = useState<ProviderAccountSummary[]>([]);
  const [selectedInstanceId, setSelectedInstanceId] = useState<string | null>(null);
  const [selectedAccountId, setSelectedAccountId] = useState<string | null>(null);
  const [error, setError] = useState<ReturnType<typeof normalizeError> | null>(null);
  const instancesGeneration = useRef(0);
  const accountsGeneration = useRef(0);
  const selectedInstanceIdRef = useRef<string | null>(null);

  const refreshInstances = useCallback(
    async (preferredInstanceId?: string, expectedInstanceId?: string) => {
      if (expectedInstanceId !== undefined && expectedInstanceId !== selectedInstanceIdRef.current)
        return;
      const generation = ++instancesGeneration.current;
      try {
        const response = await api.listInstances();
        if (generation !== instancesGeneration.current) return;
        setInstances(response.items);
        const preferred = preferredInstanceId ?? selectedInstanceIdRef.current;
        const nextInstanceId =
          preferred !== null && response.items.some(({ id }) => id === preferred)
            ? preferred
            : (response.items[0]?.id ?? null);
        if (nextInstanceId !== selectedInstanceIdRef.current) {
          selectedInstanceIdRef.current = nextInstanceId;
          accountsGeneration.current += 1;
          setAccounts([]);
          setSelectedAccountId(null);
        }
        setSelectedInstanceId(nextInstanceId);
        setError(null);
      } catch (cause) {
        if (generation === instancesGeneration.current) {
          setError(normalizeError(cause, "Unable to load Provider instances"));
        }
      }
    },
    [api]
  );

  useEffect(() => {
    if (route !== "/" && route !== "/providers") return;
    void Promise.resolve().then(() => refreshInstances());
    return () => {
      instancesGeneration.current += 1;
    };
  }, [refreshInstances, route]);

  const selectedInstance = useMemo(
    () => instances.find(({ id }) => id === selectedInstanceId) ?? null,
    [instances, selectedInstanceId]
  );

  const refreshAccounts = useCallback(
    async (expectedInstanceId: string | null) => {
      if (expectedInstanceId !== selectedInstanceIdRef.current) return;
      const generation = ++accountsGeneration.current;
      if (expectedInstanceId === null) {
        setAccounts([]);
        setSelectedAccountId(null);
        return;
      }
      try {
        const response = await api.listAccounts(expectedInstanceId);
        if (
          generation !== accountsGeneration.current ||
          expectedInstanceId !== selectedInstanceIdRef.current
        )
          return;
        setAccounts(response.items);
        setSelectedAccountId((current) => {
          if (current !== null && response.items.some(({ id }) => id === current)) return current;
          return (
            response.items.find(({ isDefault }) => isDefault)?.id ?? response.items[0]?.id ?? null
          );
        });
        setError(null);
      } catch (cause) {
        if (
          generation === accountsGeneration.current &&
          expectedInstanceId === selectedInstanceIdRef.current
        ) {
          setError(normalizeError(cause, "Unable to load Provider accounts"));
        }
      }
    },
    [api]
  );

  useEffect(() => {
    void Promise.resolve().then(() => refreshAccounts(selectedInstanceId));
    return () => {
      accountsGeneration.current += 1;
    };
  }, [refreshAccounts, selectedInstanceId]);

  const selectInstance = (instanceId: string) => {
    selectedInstanceIdRef.current = instanceId;
    accountsGeneration.current += 1;
    setAccounts([]);
    setSelectedAccountId(null);
    setSelectedInstanceId(instanceId);
  };

  if (route !== "/" && route !== "/providers") {
    return (
      <section className="view empty-view">
        <h2>Route unavailable</h2>
        <p>The host requested an unsupported Provider route.</p>
      </section>
    );
  }

  const selectedAccount = accounts.find(({ id }) => id === selectedAccountId) ?? null;

  return (
    <main className="provider-center" aria-labelledby="provider-center-title">
      <header className="provider-center-header">
        <div>
          <p className="eyebrow">GitHub · GitLab</p>
          <h1 id="provider-center-title">Providers</h1>
          <p className="muted">
            Manage provider instances, accounts, repository discovery, and local remote bindings.
          </p>
        </div>
        <button
          type="button"
          onClick={() => void refreshInstances(selectedInstanceId ?? undefined)}
        >
          Refresh
        </button>
      </header>
      {error === null ? null : <ErrorNotice error={error} />}
      <div className="provider-grid">
        <InstancePanel
          api={api}
          instances={instances}
          selectedInstanceId={selectedInstanceId}
          onSelect={selectInstance}
          onRefresh={refreshInstances}
        />
        <AccountPanel
          key={selectedInstance?.id ?? "no-provider-instance"}
          api={api}
          instance={selectedInstance}
          accounts={accounts}
          selectedAccountId={selectedAccountId}
          onSelect={(accountId) => {
            if (selectedInstance?.id === selectedInstanceIdRef.current) {
              setSelectedAccountId(accountId);
            }
          }}
          onRefresh={refreshAccounts}
        />
        <RepositoryBrowser api={api} account={selectedAccount} />
        <RemoteBindings
          api={api}
          instance={selectedInstance}
          account={selectedAccount}
          accounts={accounts}
        />
      </div>
    </main>
  );
}
