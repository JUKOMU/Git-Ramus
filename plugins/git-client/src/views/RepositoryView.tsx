import type {
  DiffResult,
  ErrorEnvelope,
  GitContextRequest,
  ParsedChangeEntry,
  Repository
} from "@git-ramus/contracts";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { GitClientApi } from "../api";
import { normalizeError } from "../api";
import { ChangeList } from "../components/ChangeList";
import { IdentityPicker } from "../components/IdentityPicker";
import { RepositoryNetworkPanel } from "../components/RepositoryNetworkPanel";
import { RepositoryIdentityBinding } from "../components/RepositoryIdentityBinding";

export type RepositoryApi = Pick<
  GitClientApi,
  | "getRepositorySnapshot"
  | "getRepositoryChanges"
  | "getRepositoryDiff"
  | "getRepositoryTrustStatus"
  | "stageRepository"
  | "unstageRepository"
  | "commitRepository"
  | "trustRepository"
  | "listIdentities"
  | "bindRepositoryIdentity"
  | "unbindRepositoryIdentity"
  | "getEffectiveRepositoryIdentity"
  | "listTransportProfiles"
  | "getEffectiveRepositoryTransport"
  | "getRepositoryNetworkState"
  | "bindRepositoryTransport"
  | "unbindRepositoryTransport"
  | "fetchRepository"
  | "pullRepository"
  | "pushRepository"
>;

interface RepositoryViewProps {
  api: RepositoryApi;
  context: GitContextRequest;
  repository: RepositorySelectionSummary;
  onBack?(): void;
}

export type RepositorySelectionSummary = Omit<Repository, "canonicalPath"> & {
  canonicalPath?: string;
};

interface DiffRequestState {
  path: string;
  staged: boolean;
}

export function RepositoryView({ api, context, repository, onBack }: RepositoryViewProps) {
  const request = useMemo(
    () => ({ ...context, repositoryId: repository.id }),
    [context, repository.id]
  );
  const [record, setRecord] = useState<Awaited<
    ReturnType<RepositoryApi["getRepositorySnapshot"]>
  > | null>(null);
  const [changes, setChanges] = useState<ParsedChangeEntry[]>([]);
  const [identities, setIdentities] = useState<
    Awaited<ReturnType<RepositoryApi["listIdentities"]>>["identities"]
  >([]);
  const [globalIdentityProfileId, setGlobalIdentityProfileId] = useState<string | null>(null);
  const [effectiveIdentity, setEffectiveIdentity] = useState<Awaited<
    ReturnType<RepositoryApi["getEffectiveRepositoryIdentity"]>
  > | null>(null);
  const [selectedIdentityProfileId, setSelectedIdentityProfileId] = useState<string | null>(null);
  const [repositoryIdentityProfileId, setRepositoryIdentityProfileId] = useState<string | null>(
    null
  );
  const [identityBindingOperation, setIdentityBindingOperation] = useState<
    "bind" | "unbind" | null
  >(null);
  const [stagedSelection, setStagedSelection] = useState<string[]>([]);
  const [unstagedSelection, setUnstagedSelection] = useState<string[]>([]);
  const [untrackedSelection, setUntrackedSelection] = useState<string[]>([]);
  const [conflictSelection, setConflictSelection] = useState<string[]>([]);
  const [trusted, setTrusted] = useState<boolean | null>(null);
  const [confirmingTrust, setConfirmingTrust] = useState(false);
  const [message, setMessage] = useState("");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [refreshError, setRefreshError] = useState<ErrorEnvelope | null>(null);
  const [trustStatusError, setTrustStatusError] = useState<ErrorEnvelope | null>(null);
  const [identityError, setIdentityError] = useState<ErrorEnvelope | null>(null);
  const [actionError, setActionError] = useState<ErrorEnvelope | null>(null);
  const [diff, setDiff] = useState<DiffResult | null>(null);
  const [diffError, setDiffError] = useState<ErrorEnvelope | null>(null);
  const [diffRequest, setDiffRequest] = useState<DiffRequestState | null>(null);
  const diffGeneration = useRef(0);
  const refreshGeneration = useRef(0);
  const trustStatusGeneration = useRef(0);
  const identityLifecycleGeneration = useRef(0);
  const identityLoadGeneration = useRef(0);
  const identityBindingBusyRef = useRef(false);

  const groupedChanges = useMemo(() => groupChanges(changes), [changes]);

  const refresh = useCallback(async () => {
    const generation = ++refreshGeneration.current;
    setLoading(true);
    try {
      const [nextRecord, nextChanges] = await Promise.all([
        api.getRepositorySnapshot(request),
        api.getRepositoryChanges(request)
      ]);
      if (generation !== refreshGeneration.current) return;
      setRecord(nextRecord);
      setChanges(nextChanges.changes);
      const grouped = groupChanges(nextChanges.changes);
      setStagedSelection((current) => retainSelection(current, grouped.staged));
      setUnstagedSelection((current) => retainSelection(current, grouped.unstaged));
      setUntrackedSelection((current) => retainSelection(current, grouped.untracked));
      setConflictSelection((current) => retainSelection(current, grouped.conflicts));
      setRefreshError(null);
    } catch (reason: unknown) {
      if (generation !== refreshGeneration.current) return;
      setRefreshError(normalizeError(reason, "Repository status could not be refreshed."));
    } finally {
      if (generation === refreshGeneration.current) {
        setLoading(false);
      }
    }
  }, [api, request]);

  useEffect(() => {
    let active = true;
    void Promise.resolve().then(() => {
      if (active) void refresh();
    });
    return () => {
      active = false;
      refreshGeneration.current += 1;
    };
  }, [refresh]);

  const loadTrustStatus = useCallback(async () => {
    const generation = ++trustStatusGeneration.current;
    setTrusted(null);
    setTrustStatusError(null);
    try {
      const result = await api.getRepositoryTrustStatus(request);
      if (generation === trustStatusGeneration.current) {
        setTrusted(result.trusted);
      }
    } catch (reason: unknown) {
      if (generation === trustStatusGeneration.current) {
        setTrustStatusError(normalizeError(reason, "Repository Trust status could not be loaded."));
      }
    }
  }, [api, request]);

  useEffect(() => {
    void Promise.resolve().then(loadTrustStatus);
    return () => {
      trustStatusGeneration.current += 1;
    };
  }, [loadTrustStatus]);

  const loadIdentityState = useCallback(
    async (lifecycle: number) => {
      const load = ++identityLoadGeneration.current;
      if (lifecycle === identityLifecycleGeneration.current) {
        setIdentityError(null);
      }
      try {
        const [identityResult, effective] = await Promise.all([
          api.listIdentities(),
          api.getEffectiveRepositoryIdentity(request)
        ]);
        if (
          lifecycle !== identityLifecycleGeneration.current ||
          load !== identityLoadGeneration.current
        ) {
          return;
        }
        setIdentities(identityResult.identities);
        setGlobalIdentityProfileId(identityResult.globalIdentityProfileId);
        setEffectiveIdentity(effective);
        setSelectedIdentityProfileId(effective.profileId);
        setRepositoryIdentityProfileId(
          effective.source === "repositoryProfile" ? effective.profileId : null
        );
        setIdentityError(null);
      } catch (reason: unknown) {
        if (
          lifecycle === identityLifecycleGeneration.current &&
          load === identityLoadGeneration.current
        ) {
          setIdentityError(normalizeError(reason, "Commit identity could not be loaded."));
        }
      }
    },
    [api, request]
  );

  useEffect(() => {
    const lifecycle = ++identityLifecycleGeneration.current;
    identityBindingBusyRef.current = false;
    void Promise.resolve().then(() => {
      if (lifecycle !== identityLifecycleGeneration.current) return;
      setIdentities([]);
      setGlobalIdentityProfileId(null);
      setEffectiveIdentity(null);
      setSelectedIdentityProfileId(null);
      setRepositoryIdentityProfileId(null);
      setIdentityBindingOperation(null);
      setIdentityError(null);
      setActionError(null);
      void loadIdentityState(lifecycle);
    });
    return () => {
      if (lifecycle === identityLifecycleGeneration.current) {
        identityLifecycleGeneration.current += 1;
      }
      identityLoadGeneration.current += 1;
      identityBindingBusyRef.current = false;
    };
  }, [loadIdentityState]);

  const bindRepositoryIdentity = async () => {
    const profileId = repositoryIdentityProfileId;
    const boundProfileId =
      effectiveIdentity?.source === "repositoryProfile" ? effectiveIdentity.profileId : null;
    if (profileId === null || profileId === boundProfileId || identityBindingBusyRef.current) {
      return;
    }
    identityBindingBusyRef.current = true;
    setIdentityBindingOperation("bind");
    setIdentityError(null);
    setActionError(null);
    const lifecycle = identityLifecycleGeneration.current;
    try {
      await api.bindRepositoryIdentity({ ...request, identityProfileId: profileId });
      if (lifecycle !== identityLifecycleGeneration.current) return;
      await loadIdentityState(lifecycle);
    } catch (reason: unknown) {
      if (lifecycle === identityLifecycleGeneration.current) {
        setActionError(normalizeError(reason, "Repository identity could not be bound."));
      }
    } finally {
      if (lifecycle === identityLifecycleGeneration.current) {
        identityBindingBusyRef.current = false;
        setIdentityBindingOperation(null);
      }
    }
  };

  const unbindRepositoryIdentity = async () => {
    const boundProfileId =
      effectiveIdentity?.source === "repositoryProfile" ? effectiveIdentity.profileId : null;
    if (boundProfileId === null || identityBindingBusyRef.current) return;
    identityBindingBusyRef.current = true;
    setIdentityBindingOperation("unbind");
    setIdentityError(null);
    setActionError(null);
    const lifecycle = identityLifecycleGeneration.current;
    try {
      await api.unbindRepositoryIdentity(request);
      if (lifecycle !== identityLifecycleGeneration.current) return;
      await loadIdentityState(lifecycle);
    } catch (reason: unknown) {
      if (lifecycle === identityLifecycleGeneration.current) {
        setActionError(normalizeError(reason, "Repository identity could not be unbound."));
      }
    } finally {
      if (lifecycle === identityLifecycleGeneration.current) {
        identityBindingBusyRef.current = false;
        setIdentityBindingOperation(null);
      }
    }
  };

  const showDiff = async (change: ParsedChangeEntry, staged: boolean) => {
    const generation = ++diffGeneration.current;
    const nextRequest = { path: change.path, staged };
    setDiffRequest(nextRequest);
    setDiff(null);
    setDiffError(null);
    try {
      const nextDiff = await api.getRepositoryDiff({
        ...request,
        paths: [change.path],
        staged
      });
      if (generation === diffGeneration.current) {
        setDiff(nextDiff);
      }
    } catch (reason: unknown) {
      if (generation === diffGeneration.current) {
        setDiffError(normalizeError(reason, "Diff could not be loaded."));
      }
    }
  };

  const retryDiff = () => {
    if (diffRequest === null) return;
    const change = changes.find((candidate) => candidate.path === diffRequest.path);
    if (change !== undefined) void showDiff(change, diffRequest.staged);
  };

  const invalidateDiff = () => {
    diffGeneration.current += 1;
    setDiff(null);
    setDiffError(null);
    setDiffRequest(null);
  };

  const confirmTrust = async () => {
    setBusy(true);
    setActionError(null);
    try {
      await api.trustRepository(request);
      setTrusted(true);
      setTrustStatusError(null);
      setConfirmingTrust(false);
    } catch (reason: unknown) {
      setActionError(normalizeError(reason, "Repository Trust could not be recorded."));
    } finally {
      setBusy(false);
    }
  };

  const recoverWriteFailure = async (reason: unknown, fallbackMessage: string) => {
    const nextError = normalizeError(reason, fallbackMessage);
    setActionError(nextError);
    if (nextError.code === "git.trust-required") {
      setTrusted(false);
      setConfirmingTrust(false);
    }
    await refresh();
  };

  const stage = async (paths: string[], all: boolean) => {
    if (trusted !== true || (!all && paths.length === 0)) return;
    setBusy(true);
    setActionError(null);
    try {
      await api.stageRepository({ ...request, paths, all });
      invalidateDiff();
      await refresh();
      invalidateDiff();
    } catch (reason: unknown) {
      await recoverWriteFailure(reason, "Changes could not be staged.");
    } finally {
      setBusy(false);
    }
  };

  const unstage = async () => {
    if (trusted !== true || stagedSelection.length === 0) return;
    setBusy(true);
    setActionError(null);
    try {
      await api.unstageRepository({ ...request, paths: stagedSelection });
      invalidateDiff();
      await refresh();
      invalidateDiff();
    } catch (reason: unknown) {
      await recoverWriteFailure(reason, "Changes could not be unstaged.");
    } finally {
      setBusy(false);
    }
  };

  const commit = async () => {
    const commitMessage = message.trim();
    if (trusted !== true || groupedChanges.staged.length === 0 || !commitMessage) return;
    setBusy(true);
    setActionError(null);
    try {
      await api.commitRepository({
        ...request,
        message: commitMessage,
        identityProfileId: selectedIdentityProfileId
      });
      setMessage("");
      invalidateDiff();
      await refresh();
      invalidateDiff();
    } catch (reason: unknown) {
      await recoverWriteFailure(reason, "Commit could not be created.");
    } finally {
      setBusy(false);
    }
  };

  const stageSelected = Array.from(
    new Set([...unstagedSelection, ...untrackedSelection, ...conflictSelection])
  );
  const canCommit =
    trusted === true &&
    !busy &&
    identityBindingOperation === null &&
    effectiveIdentity !== null &&
    message.trim().length > 0 &&
    groupedChanges.staged.length > 0;

  return (
    <section className="view repository-view">
      <header className="view-header">
        <div>
          {onBack === undefined ? null : (
            <button className="button-link" type="button" onClick={onBack}>
              Back
            </button>
          )}
          <p className="eyebrow">Repository</p>
          <h2>{repository.displayName}</h2>
          <p className="path">
            {record?.repository.canonicalPath ??
              repository.canonicalPath ??
              "Local path managed by the host"}
          </p>
        </div>
        <div className="repository-status">
          <span>{record?.snapshot?.branch ?? "Branch unknown"}</span>
          {trusted === true ? (
            <span className="success-notice">Trusted on this device</span>
          ) : trusted === false ? (
            <button type="button" disabled={busy} onClick={() => setConfirmingTrust(true)}>
              Trust repository
            </button>
          ) : trustStatusError === null ? (
            <span>Checking repository Trust…</span>
          ) : null}
        </div>
      </header>

      {confirmingTrust && trusted === false ? (
        <div
          className="trust-confirmation"
          role="alertdialog"
          aria-label="Confirm repository Trust"
        >
          <p>Trust allows write operations and repository-local Git configuration.</p>
          <button type="button" disabled={busy} onClick={() => void confirmTrust()}>
            Confirm trust
          </button>
          <button type="button" disabled={busy} onClick={() => setConfirmingTrust(false)}>
            Cancel
          </button>
        </div>
      ) : null}
      {refreshError === null ? null : (
        <ErrorNotice error={refreshError} onRetry={() => void refresh()} />
      )}
      {trustStatusError === null ? null : (
        <ErrorNotice error={trustStatusError} onRetry={() => void loadTrustStatus()} />
      )}
      {identityError === null ? null : (
        <ErrorNotice
          error={identityError}
          onRetry={() => void loadIdentityState(identityLifecycleGeneration.current)}
        />
      )}
      {actionError === null ? null : (
        <ErrorNotice error={actionError} onRetry={() => void refresh()} />
      )}
      {loading ? <p>Loading repository…</p> : null}

      <RepositoryNetworkPanel
        api={api}
        repository={repository}
        context={context}
        trusted={trusted === true}
        onCompleted={refresh}
      />

      <div className="repository-layout">
        <div className="changes-panel">
          <div className="panel-heading">
            <h2>Changes</h2>
            <div className="button-row">
              <button
                type="button"
                disabled={
                  trusted !== true ||
                  busy ||
                  groupedChanges.unstaged.length +
                    groupedChanges.untracked.length +
                    groupedChanges.conflicts.length ===
                    0
                }
                onClick={() => void stage([], true)}
              >
                Stage all
              </button>
              <button
                type="button"
                disabled={trusted !== true || busy || stageSelected.length === 0}
                onClick={() => void stage(stageSelected, false)}
              >
                Stage selected
              </button>
              <button
                type="button"
                disabled={trusted !== true || busy || stagedSelection.length === 0}
                onClick={() => void unstage()}
              >
                Unstage selected
              </button>
            </div>
          </div>
          <ChangeList
            title="Staged"
            changes={groupedChanges.staged}
            selectedPaths={stagedSelection}
            onSelectionChange={setStagedSelection}
            onViewDiff={(change) => void showDiff(change, true)}
          />
          <ChangeList
            title="Unstaged"
            changes={groupedChanges.unstaged}
            selectedPaths={unstagedSelection}
            onSelectionChange={setUnstagedSelection}
            onViewDiff={(change) => void showDiff(change, false)}
          />
          <ChangeList
            title="Untracked"
            changes={groupedChanges.untracked}
            selectedPaths={untrackedSelection}
            onSelectionChange={setUntrackedSelection}
            onViewDiff={(change) => void showDiff(change, false)}
          />
          <ChangeList
            title="Conflicts"
            changes={groupedChanges.conflicts}
            selectedPaths={conflictSelection}
            onSelectionChange={setConflictSelection}
            onViewDiff={(change) => void showDiff(change, false)}
          />
        </div>

        <aside className="detail-panel">
          <section className="diff-panel">
            <h2>Diff</h2>
            {diffError === null ? null : <ErrorNotice error={diffError} onRetry={retryDiff} />}
            {diff === null && diffError === null ? (
              <p>Select a changed path to inspect its diff.</p>
            ) : null}
            {diff?.summary.files.map((file) => (
              <article key={file.path}>
                <h3>{file.path}</h3>
                <p className="muted">
                  {file.binary
                    ? "Binary file"
                    : `${file.additions ?? "Unknown"} additions · ${
                        file.deletions ?? "Unknown"
                      } deletions`}
                </p>
              </article>
            ))}
            {diff !== null && diff.contentUnavailableReason !== null ? (
              <p>{diffContentUnavailableMessage(diff.contentUnavailableReason)}</p>
            ) : null}
            {diff?.patch === null && diff.contentUnavailableReason === null ? (
              <p>No textual diff content was returned.</p>
            ) : null}
            {diff !== null && diff.patch !== null ? (
              <pre aria-label="Diff patch">{diff.patch}</pre>
            ) : null}
            {diff?.truncated ? (
              <p className="muted">Diff content was truncated at the safe display limit.</p>
            ) : null}
          </section>

          <section className="commit-panel">
            <h2>Commit</h2>
            <IdentityPicker
              identities={identities}
              globalIdentityProfileId={globalIdentityProfileId}
              effectiveIdentity={effectiveIdentity}
              selectedIdentityProfileId={selectedIdentityProfileId}
              onChange={setSelectedIdentityProfileId}
            />
            <RepositoryIdentityBinding
              identities={identities}
              boundIdentityProfileId={
                effectiveIdentity?.source === "repositoryProfile"
                  ? effectiveIdentity.profileId
                  : null
              }
              selectedIdentityProfileId={repositoryIdentityProfileId}
              operation={identityBindingOperation}
              onSelectionChange={setRepositoryIdentityProfileId}
              onBind={() => void bindRepositoryIdentity()}
              onUnbind={() => void unbindRepositoryIdentity()}
            />
            <label>
              Commit message
              <textarea
                aria-label="Commit message"
                value={message}
                onChange={(event) => setMessage(event.target.value)}
              />
            </label>
            <p className="muted">Commits all changes currently staged in the Git index.</p>
            <button type="button" disabled={!canCommit} onClick={() => void commit()}>
              Commit staged changes
            </button>
          </section>
        </aside>
      </div>
    </section>
  );
}

function ErrorNotice({ error, onRetry }: { error: ErrorEnvelope; onRetry(): void }) {
  const actions =
    error.recoveryActions.length > 0
      ? error.recoveryActions
      : error.retryable
        ? [{ id: "retry", label: "Try again", kind: "retry" as const }]
        : [];
  return (
    <div className="error-notice" role="alert">
      <p>{error.message}</p>
      {actions.map((action) => (
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

function diffContentUnavailableMessage(
  reason: NonNullable<DiffResult["contentUnavailableReason"]>
): string {
  const messages: Record<NonNullable<DiffResult["contentUnavailableReason"]>, string> = {
    binary: "Binary diff content is not displayed.",
    untrustedRepository: "Trust the repository to view diff content.",
    nonUtf8Content: "Diff content is not valid UTF-8.",
    outputLimit: "Diff content exceeded the safe output limit.",
    untrackedContentUnavailable: "Untracked file content is unavailable."
  };
  return messages[reason];
}

function groupChanges(changes: ParsedChangeEntry[]) {
  const conflicts = changes.filter((change) => change.conflicted);
  const untracked = changes.filter((change) => !change.conflicted && change.kind === "untracked");
  const regular = changes.filter((change) => !change.conflicted && change.kind !== "untracked");
  return {
    staged: regular.filter((change) => change.staged),
    unstaged: regular.filter((change) => change.unstaged),
    untracked,
    conflicts
  };
}

function retainSelection(selectedPaths: string[], changes: ParsedChangeEntry[]) {
  const availablePaths = new Set(changes.map((change) => change.path));
  return selectedPaths.filter((path) => availablePaths.has(path));
}
