import {
  gitBranchNameSchema,
  type EffectiveTransport,
  type ErrorEnvelope,
  type GitContextRequest,
  type NetworkOperationResult,
  type RepositoryNetworkState,
  type TransportProfileSummary
} from "@git-ramus/contracts";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { GitClientApi } from "../api";
import { normalizeError } from "../api";

export type RepositoryNetworkApi = Pick<
  GitClientApi,
  | "listTransportProfiles"
  | "getEffectiveRepositoryTransport"
  | "getRepositoryNetworkState"
  | "bindRepositoryTransport"
  | "unbindRepositoryTransport"
  | "fetchRepository"
  | "pullRepository"
  | "pushRepository"
>;

interface RepositoryNetworkPanelProps {
  api: RepositoryNetworkApi;
  repository: { id: string; displayName: string };
  context: GitContextRequest;
  trusted: boolean;
  onCompleted(): void | Promise<void>;
}

type NetworkAction = "fetch" | "pull" | "push";

export function RepositoryNetworkPanel({
  api,
  repository,
  context,
  trusted,
  onCompleted
}: RepositoryNetworkPanelProps) {
  const request = useMemo(
    () => ({ ...context, repositoryId: repository.id }),
    [context, repository.id]
  );
  const [profiles, setProfiles] = useState<TransportProfileSummary[]>([]);
  const [effective, setEffective] = useState<EffectiveTransport | null>(null);
  const [network, setNetwork] = useState<RepositoryNetworkState | null>(null);
  const [selectedProfileId, setSelectedProfileId] = useState("");
  const [fetchRemote, setFetchRemote] = useState("");
  const [pushTargetOpen, setPushTargetOpen] = useState(false);
  const [pushRemote, setPushRemote] = useState("");
  const [pushBranch, setPushBranch] = useState("");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<NetworkAction | "transport" | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<ErrorEnvelope | null>(null);
  const [errorPhase, setErrorPhase] = useState<"load" | "operation" | null>(null);
  const lifecycleRef = useRef(0);
  const loadRef = useRef(0);
  const operationRef = useRef(0);
  const busyRef = useRef(false);
  const abortRef = useRef<AbortController | null>(null);
  const retryRef = useRef<(() => void) | null>(null);

  const load = useCallback(
    async (lifecycle: number, showLoading: boolean) => {
      const loadId = ++loadRef.current;
      if (showLoading) setLoading(true);
      try {
        const [profileResult, nextEffective, nextNetwork] = await Promise.all([
          api.listTransportProfiles(),
          api.getEffectiveRepositoryTransport(request),
          api.getRepositoryNetworkState(request)
        ]);
        if (lifecycle !== lifecycleRef.current || loadId !== loadRef.current) return;
        setProfiles(profileResult.items);
        setEffective(nextEffective);
        setNetwork(nextNetwork);
        setSelectedProfileId(nextEffective.profile?.id ?? "");
        const firstRemote = nextNetwork.remotes[0]?.name ?? "";
        setFetchRemote((current) =>
          nextNetwork.remotes.some((remote) => remote.name === current) ? current : firstRemote
        );
        setPushRemote((current) =>
          nextNetwork.remotes.some((remote) => remote.name === current) ? current : firstRemote
        );
        setPushBranch((current) => current || nextNetwork.branch || "");
        if (nextNetwork.upstream !== null) setPushTargetOpen(false);
        setError(null);
        setErrorPhase(null);
        retryRef.current = null;
      } catch (reason: unknown) {
        if (lifecycle === lifecycleRef.current && loadId === loadRef.current) {
          setError(normalizeError(reason, "Repository network state could not be loaded."));
          setErrorPhase("load");
          retryRef.current = null;
        }
      } finally {
        if (lifecycle === lifecycleRef.current && loadId === loadRef.current) setLoading(false);
      }
    },
    [api, request]
  );

  useEffect(() => {
    const lifecycle = ++lifecycleRef.current;
    busyRef.current = false;
    abortRef.current?.abort();
    abortRef.current = null;
    void Promise.resolve().then(() => {
      if (lifecycle !== lifecycleRef.current) return;
      setProfiles([]);
      setEffective(null);
      setNetwork(null);
      setSelectedProfileId("");
      setFetchRemote("");
      setPushTargetOpen(false);
      setPushRemote("");
      setPushBranch("");
      setLoading(true);
      setBusy(null);
      setNotice(null);
      setError(null);
      setErrorPhase(null);
      retryRef.current = null;
      void load(lifecycle, true);
    });
    return () => {
      if (lifecycle === lifecycleRef.current) lifecycleRef.current += 1;
      loadRef.current += 1;
      operationRef.current += 1;
      busyRef.current = false;
      abortRef.current?.abort();
      abortRef.current = null;
    };
  }, [load]);

  const finishTerminal = async (lifecycle: number) => {
    await Promise.allSettled([Promise.resolve().then(onCompleted), load(lifecycle, false)]);
  };

  const runNetwork = async (
    action: NetworkAction,
    execute: (operationId: string, signal: AbortSignal) => Promise<NetworkOperationResult | null>
  ) => {
    if (!trusted || busyRef.current) return;
    busyRef.current = true;
    setBusy(action);
    setNotice(null);
    setError(null);
    setErrorPhase(null);
    retryRef.current = null;
    const lifecycle = lifecycleRef.current;
    const operation = ++operationRef.current;
    const controller = new AbortController();
    abortRef.current = controller;
    let terminalNotice: string | null = null;
    let terminalError: ErrorEnvelope | null = null;
    try {
      const result = await execute(crypto.randomUUID(), controller.signal);
      if (lifecycle !== lifecycleRef.current || operation !== operationRef.current) return;
      if (result === null) terminalNotice = "Network operation cancelled.";
      else {
        setNetwork(result.networkState);
        if (action === "push") setPushTargetOpen(false);
        terminalNotice = `${capitalize(action)} completed.`;
      }
    } catch (reason: unknown) {
      if (lifecycle !== lifecycleRef.current || operation !== operationRef.current) return;
      if (isAbortError(reason)) terminalNotice = "Network operation cancelled.";
      else terminalError = normalizeError(reason, `${capitalize(action)} could not be completed.`);
    }
    if (lifecycle === lifecycleRef.current && operation === operationRef.current) {
      await finishTerminal(lifecycle);
      if (lifecycle === lifecycleRef.current && operation === operationRef.current) {
        setNotice(terminalNotice);
        if (terminalError !== null) {
          retryRef.current = () => void runNetwork(action, execute);
          setErrorPhase("operation");
          setError(terminalError);
        }
        busyRef.current = false;
        abortRef.current = null;
        setBusy(null);
      }
    }
  };

  const runTransportMutation = async (execute: () => Promise<unknown>, success: string) => {
    if (!trusted || busyRef.current) return;
    busyRef.current = true;
    setBusy("transport");
    setNotice(null);
    setError(null);
    setErrorPhase(null);
    retryRef.current = null;
    const lifecycle = lifecycleRef.current;
    const operation = ++operationRef.current;
    let terminalNotice: string | null = null;
    let terminalError: ErrorEnvelope | null = null;
    try {
      const result = await execute();
      if (lifecycle !== lifecycleRef.current || operation !== operationRef.current) return;
      terminalNotice = result === null ? "Transport change cancelled." : success;
    } catch (reason: unknown) {
      if (lifecycle !== lifecycleRef.current || operation !== operationRef.current) return;
      terminalError = normalizeError(reason, "Repository transport could not be changed.");
    }
    if (lifecycle === lifecycleRef.current && operation === operationRef.current) {
      await finishTerminal(lifecycle);
      if (lifecycle === lifecycleRef.current && operation === operationRef.current) {
        setNotice(terminalNotice);
        if (terminalError !== null) {
          retryRef.current = () => void runTransportMutation(execute, success);
          setErrorPhase("operation");
          setError(terminalError);
        }
        busyRef.current = false;
        setBusy(null);
      }
    }
  };

  const pullUnsafe =
    network === null ||
    network.detached ||
    network.upstream === null ||
    network.conflictedCount > 0 ||
    network.inProgress !== null ||
    (network.ahead > 0 && network.behind > 0);
  const writesDisabled = !trusted || loading || busy !== null || network === null;
  const primaryKind =
    network?.remotes.find((remote) => remote.name === "origin")?.kind ??
    network?.remotes[0]?.kind ??
    "unknown";
  const compatibleProfiles = profiles.filter(
    (profile) => profile.available && profile.kind === primaryKind
  );
  const selectedProfileValid = compatibleProfiles.some(
    (profile) => profile.id === selectedProfileId
  );
  const selectedProfileIsCurrent =
    effective?.source === "profile" && effective.profile?.id === selectedProfileId;
  const pushTargetValid =
    pushRemote.length > 0 && gitBranchNameSchema.safeParse(pushBranch).success;

  return (
    <section className="card repository-network-panel" aria-labelledby="repository-network-title">
      <header>
        <div>
          <p className="eyebrow">Remote operations</p>
          <h2 id="repository-network-title">Network</h2>
        </div>
        {network === null ? null : (
          <span className="badge">
            {network.ahead} ahead · {network.behind} behind
          </span>
        )}
      </header>
      {loading ? <p>Loading repository network state…</p> : null}
      {error === null ? null : (
        <NetworkErrorNotice
          error={error}
          onRetry={() => {
            if (errorPhase === "operation") retryRef.current?.();
            else void load(lifecycleRef.current, false);
          }}
        />
      )}
      {notice === null ? null : <p className="success-notice">{notice}</p>}

      {network === null ? null : (
        <>
          <p className="network-upstream">
            {network.upstream === null
              ? "No upstream configured"
              : `Upstream: ${network.upstream.remoteName}/${network.upstream.branchName}`}
          </p>
          <div className="network-remotes" aria-label="Repository remotes">
            {network.remotes.map((remote) => (
              <article key={remote.name}>
                <strong>{remote.name}</strong>
                <span className="secondary-line">{remote.kind.toUpperCase()} transport</span>
                <span className="secondary-line">{remote.fetchUrl}</span>
                {remote.pushUrl === null || remote.pushUrl === remote.fetchUrl ? null : (
                  <span className="secondary-line">Push: {remote.pushUrl}</span>
                )}
              </article>
            ))}
          </div>

          <div className="network-operation-grid">
            <label>
              Fetch remote
              <select
                aria-label="Fetch remote"
                disabled={writesDisabled}
                value={fetchRemote}
                onChange={(event) => setFetchRemote(event.target.value)}
              >
                {network.remotes.map((remote) => (
                  <option key={remote.name} value={remote.name}>
                    {remote.name}
                  </option>
                ))}
              </select>
            </label>
            <button
              type="button"
              disabled={writesDisabled || fetchRemote.length === 0}
              onClick={() =>
                void runNetwork("fetch", (operationId, signal) =>
                  api.fetchRepository({ ...request, operationId, remoteName: fetchRemote }, signal)
                )
              }
            >
              Fetch
            </button>
            <button
              type="button"
              disabled={writesDisabled || pullUnsafe}
              onClick={() =>
                void runNetwork("pull", (operationId, signal) =>
                  api.pullRepository({ ...request, operationId }, signal)
                )
              }
            >
              Pull
            </button>
            <button
              type="button"
              disabled={writesDisabled || network.detached || network.inProgress !== null}
              onClick={() => {
                if (network.upstream === null) setPushTargetOpen(true);
                else {
                  void runNetwork("push", (operationId, signal) =>
                    api.pushRepository({ ...request, operationId, target: null }, signal)
                  );
                }
              }}
            >
              Push
            </button>
          </div>
          {network.ahead > 0 && network.behind > 0 ? (
            <p className="signing-validation">
              Pull is fast-forward only; local and remote histories have diverged.
            </p>
          ) : null}
          {pushTargetOpen && network.upstream === null ? (
            <div className="network-push-target">
              <label>
                Remote
                <select
                  aria-label="Remote"
                  disabled={writesDisabled}
                  value={pushRemote}
                  onChange={(event) => setPushRemote(event.target.value)}
                >
                  {network.remotes.map((remote) => (
                    <option key={remote.name} value={remote.name}>
                      {remote.name}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                Remote branch
                <input
                  aria-label="Remote branch"
                  maxLength={1024}
                  disabled={writesDisabled}
                  value={pushBranch}
                  onChange={(event) => setPushBranch(event.target.value)}
                />
              </label>
              <div className="button-row">
                <button
                  type="button"
                  disabled={writesDisabled || !pushTargetValid}
                  onClick={() =>
                    void runNetwork("push", (operationId, signal) =>
                      api.pushRepository(
                        {
                          ...request,
                          operationId,
                          target: { remoteName: pushRemote, branchName: pushBranch }
                        },
                        signal
                      )
                    )
                  }
                >
                  Set upstream and push
                </button>
                <button
                  className="button-link"
                  type="button"
                  disabled={busy !== null}
                  onClick={() => setPushTargetOpen(false)}
                >
                  Cancel push
                </button>
              </div>
            </div>
          ) : null}
        </>
      )}

      {busy === "fetch" || busy === "pull" || busy === "push" ? (
        <button type="button" onClick={() => abortRef.current?.abort()}>
          Cancel network operation
        </button>
      ) : null}

      <section className="repository-transport-settings">
        <h3>Transport profile</h3>
        <p className="muted">
          {effective?.source === "profile"
            ? `Using ${effective.profile?.displayName ?? "a saved profile"}`
            : "System Git configuration"}
        </p>
        <label>
          Repository transport profile
          <select
            aria-label="Repository transport profile"
            disabled={!trusted || loading || busy !== null || compatibleProfiles.length === 0}
            value={selectedProfileId}
            onChange={(event) => setSelectedProfileId(event.target.value)}
          >
            <option value="">Choose a compatible profile</option>
            {compatibleProfiles.map((profile) => (
              <option key={profile.id} value={profile.id}>
                {profile.displayName}
              </option>
            ))}
          </select>
        </label>
        <div className="button-row repository-transport-actions">
          <button
            type="button"
            disabled={
              !trusted ||
              loading ||
              busy !== null ||
              !selectedProfileValid ||
              selectedProfileIsCurrent
            }
            onClick={() =>
              void runTransportMutation(
                () =>
                  api.bindRepositoryTransport({
                    ...request,
                    transportProfileId: selectedProfileId,
                    replaceExisting: true
                  }),
                "Transport profile applied."
              )
            }
          >
            Apply transport profile
          </button>
          {effective?.source === "profile" && effective.driftStatus === "clean" ? (
            <button
              className="button-link"
              type="button"
              disabled={!trusted || busy !== null}
              onClick={() =>
                void runTransportMutation(
                  () => api.unbindRepositoryTransport({ ...request, driftResolution: "reject" }),
                  "Transport profile unbound."
                )
              }
            >
              Unbind profile
            </button>
          ) : null}
        </div>
        {effective?.source === "profile" && effective.driftStatus === "drifted" ? (
          <div className="transport-drift-actions">
            <p className="signing-validation">Transport configuration drift detected.</p>
            <div className="button-row">
              <button
                type="button"
                disabled={!trusted || busy !== null}
                onClick={() =>
                  void runTransportMutation(
                    () => api.unbindRepositoryTransport({ ...request, driftResolution: "reapply" }),
                    "Transport profile reapplied."
                  )
                }
              >
                Reapply profile
              </button>
              <button
                className="button-link"
                type="button"
                disabled={!trusted || busy !== null}
                onClick={() =>
                  void runTransportMutation(
                    () =>
                      api.unbindRepositoryTransport({
                        ...request,
                        driftResolution: "keepExternal"
                      }),
                    "External transport configuration preserved."
                  )
                }
              >
                Keep external configuration
              </button>
            </div>
          </div>
        ) : null}
      </section>
    </section>
  );
}

function isAbortError(reason: unknown): boolean {
  return reason instanceof DOMException && reason.name === "AbortError";
}

function capitalize(value: string): string {
  return `${value.slice(0, 1).toUpperCase()}${value.slice(1)}`;
}

function NetworkErrorNotice({ error, onRetry }: { error: ErrorEnvelope; onRetry(): void }) {
  return (
    <div className="error-notice" role="alert">
      <p>{error.message}</p>
      {error.recoveryActions.map((action) => (
        <button
          key={action.id}
          type="button"
          disabled={action.kind !== "retry"}
          title={action.kind === "retry" ? undefined : "Complete this action in the Git-Ramus host"}
          onClick={action.kind === "retry" ? onRetry : undefined}
        >
          {action.label}
        </button>
      ))}
    </div>
  );
}
