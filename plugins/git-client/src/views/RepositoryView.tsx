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

export type RepositoryApi = Pick<
  GitClientApi,
  | "getRepositorySnapshot"
  | "getRepositoryChanges"
  | "getRepositoryDiff"
  | "stageRepository"
  | "unstageRepository"
  | "commitRepository"
  | "trustRepository"
  | "listIdentities"
  | "getEffectiveRepositoryIdentity"
>;

interface RepositoryViewProps {
  api: RepositoryApi;
  context: GitContextRequest;
  repository: Repository;
  onBack?(): void;
}

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
  const [stagedSelection, setStagedSelection] = useState<string[]>([]);
  const [unstagedSelection, setUnstagedSelection] = useState<string[]>([]);
  const [untrackedSelection, setUntrackedSelection] = useState<string[]>([]);
  const [conflictSelection, setConflictSelection] = useState<string[]>([]);
  const [trusted, setTrusted] = useState(false);
  const [confirmingTrust, setConfirmingTrust] = useState(false);
  const [message, setMessage] = useState("");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [refreshError, setRefreshError] = useState<ErrorEnvelope | null>(null);
  const [actionError, setActionError] = useState<ErrorEnvelope | null>(null);
  const [diff, setDiff] = useState<DiffResult | null>(null);
  const [diffError, setDiffError] = useState<ErrorEnvelope | null>(null);
  const [diffRequest, setDiffRequest] = useState<DiffRequestState | null>(null);
  const diffGeneration = useRef(0);

  const groupedChanges = useMemo(() => groupChanges(changes), [changes]);

  const refresh = useCallback(async () => {
    try {
      const [nextRecord, nextChanges] = await Promise.all([
        api.getRepositorySnapshot(request),
        api.getRepositoryChanges(request)
      ]);
      setRecord(nextRecord);
      setChanges(nextChanges.changes);
      const grouped = groupChanges(nextChanges.changes);
      setStagedSelection((current) => retainSelection(current, grouped.staged));
      setUnstagedSelection((current) => retainSelection(current, grouped.unstaged));
      setUntrackedSelection((current) => retainSelection(current, grouped.untracked));
      setConflictSelection((current) => retainSelection(current, grouped.conflicts));
      setRefreshError(null);
    } catch (reason: unknown) {
      setRefreshError(normalizeError(reason, "Repository status could not be refreshed."));
    } finally {
      setLoading(false);
    }
  }, [api, request]);

  useEffect(() => {
    void Promise.resolve().then(refresh);
  }, [refresh]);

  useEffect(() => {
    let active = true;
    void Promise.all([api.listIdentities(), api.getEffectiveRepositoryIdentity(request)])
      .then(([identityResult, effective]) => {
        if (!active) return;
        setIdentities(identityResult.identities);
        setGlobalIdentityProfileId(identityResult.globalIdentityProfileId);
        setEffectiveIdentity(effective);
        setSelectedIdentityProfileId(effective.profileId);
      })
      .catch((reason: unknown) => {
        if (active) {
          setActionError(normalizeError(reason, "Commit identity could not be loaded."));
        }
      });
    return () => {
      active = false;
    };
  }, [api, request]);

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

  const confirmTrust = async () => {
    setBusy(true);
    setActionError(null);
    try {
      await api.trustRepository(request);
      setTrusted(true);
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
    if (!trusted || (!all && paths.length === 0)) return;
    setBusy(true);
    setActionError(null);
    try {
      await api.stageRepository({ ...request, paths, all });
      await refresh();
    } catch (reason: unknown) {
      await recoverWriteFailure(reason, "Changes could not be staged.");
    } finally {
      setBusy(false);
    }
  };

  const unstage = async () => {
    if (!trusted || stagedSelection.length === 0) return;
    setBusy(true);
    setActionError(null);
    try {
      await api.unstageRepository({ ...request, paths: stagedSelection });
      await refresh();
    } catch (reason: unknown) {
      await recoverWriteFailure(reason, "Changes could not be unstaged.");
    } finally {
      setBusy(false);
    }
  };

  const commit = async () => {
    const commitMessage = message.trim();
    if (!trusted || groupedChanges.staged.length === 0 || !commitMessage) return;
    setBusy(true);
    setActionError(null);
    try {
      await api.commitRepository({
        ...request,
        message: commitMessage,
        identityProfileId: selectedIdentityProfileId
      });
      setMessage("");
      await refresh();
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
    trusted && !busy && message.trim().length > 0 && groupedChanges.staged.length > 0;

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
          <p className="path">{repository.canonicalPath}</p>
        </div>
        <div className="repository-status">
          <span>{record?.snapshot?.branch ?? "Branch unknown"}</span>
          {trusted ? (
            <span className="success-notice">Trusted for this session</span>
          ) : (
            <button type="button" disabled={busy} onClick={() => setConfirmingTrust(true)}>
              Trust repository
            </button>
          )}
        </div>
      </header>

      {confirmingTrust && !trusted ? (
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
      {actionError === null ? null : (
        <ErrorNotice error={actionError} onRetry={() => void refresh()} />
      )}
      {loading ? <p>Loading repository…</p> : null}

      <div className="repository-layout">
        <div className="changes-panel">
          <div className="panel-heading">
            <h2>Changes</h2>
            <div className="button-row">
              <button
                type="button"
                disabled={
                  !trusted ||
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
                disabled={!trusted || busy || stageSelected.length === 0}
                onClick={() => void stage(stageSelected, false)}
              >
                Stage selected
              </button>
              <button
                type="button"
                disabled={!trusted || busy || stagedSelection.length === 0}
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
                {file.binary ? (
                  <p>Binary diff</p>
                ) : (
                  <pre>{`${file.old ?? ""}\n${file.new ?? ""}`}</pre>
                )}
              </article>
            ))}
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
