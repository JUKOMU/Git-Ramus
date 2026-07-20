import type {
  TransportKind,
  TransportProfileDeletionImpact,
  TransportProfileDeletionResolution,
  TransportProfileSummary
} from "@git-ramus/contracts";
import { useEffect, useRef, useState } from "react";
import type { GitClientApi } from "../api";
import { normalizeError } from "../api";
import {
  TransportProfileForm,
  type TransportProfileMutation
} from "../components/TransportProfileForm";

export type TransportProfilesApi = Pick<
  GitClientApi,
  | "listTransportProfiles"
  | "createTransportProfile"
  | "updateTransportProfile"
  | "getTransportProfileDeletionImpact"
  | "deleteTransportProfile"
>;

interface TransportProfilesViewProps {
  api: TransportProfilesApi;
}

interface EditorState {
  kind: TransportKind;
  profile: TransportProfileSummary | null;
}

interface DeletionState {
  profile: TransportProfileSummary;
  impact: TransportProfileDeletionImpact | null;
  resolutions: Readonly<Record<string, string>>;
  loading: boolean;
  deleting: boolean;
}

export function TransportProfilesView({ api }: TransportProfilesViewProps) {
  const [profiles, setProfiles] = useState<TransportProfileSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [deletion, setDeletion] = useState<DeletionState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const generationRef = useRef(0);
  const loadSequenceRef = useRef(0);
  const busyRef = useRef(false);
  const refreshingRef = useRef(false);

  useEffect(() => {
    const generation = ++generationRef.current;
    const sequence = ++loadSequenceRef.current;
    busyRef.current = false;
    refreshingRef.current = false;
    void Promise.resolve().then(() => {
      if (generation !== generationRef.current) return;
      setProfiles([]);
      setLoading(true);
      setRefreshing(false);
      setBusy(false);
      setEditor(null);
      setDeletion(null);
      setError(null);
      setNotice(null);
    });
    void api
      .listTransportProfiles()
      .then((result) => {
        if (generation !== generationRef.current || sequence !== loadSequenceRef.current) return;
        setProfiles(result.items);
      })
      .catch((reason: unknown) => {
        if (generation !== generationRef.current || sequence !== loadSequenceRef.current) return;
        setError(normalizeError(reason, "Transport profiles could not be loaded.").message);
      })
      .finally(() => {
        if (generation === generationRef.current && sequence === loadSequenceRef.current) {
          setLoading(false);
        }
      });
    return () => {
      if (generation === generationRef.current) generationRef.current += 1;
    };
  }, [api]);

  const beginOperation = () => {
    if (busyRef.current) return false;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    setNotice(null);
    return true;
  };

  const finishOperation = (generation: number) => {
    if (generation !== generationRef.current) return;
    busyRef.current = false;
    setBusy(false);
  };

  const refresh = async () => {
    if (busyRef.current || refreshingRef.current) return;
    refreshingRef.current = true;
    const generation = generationRef.current;
    const sequence = ++loadSequenceRef.current;
    setRefreshing(true);
    setError(null);
    try {
      const result = await api.listTransportProfiles();
      if (generation !== generationRef.current || sequence !== loadSequenceRef.current) return;
      setProfiles(result.items);
      setNotice("Transport profiles refreshed.");
    } catch (reason: unknown) {
      if (generation !== generationRef.current || sequence !== loadSequenceRef.current) return;
      setError(normalizeError(reason, "Transport profiles could not be refreshed.").message);
    } finally {
      if (generation === generationRef.current && sequence === loadSequenceRef.current) {
        refreshingRef.current = false;
        setRefreshing(false);
        setLoading(false);
      }
    }
  };

  const saveProfile = async (request: TransportProfileMutation) => {
    if (!beginOperation()) return;
    const generation = generationRef.current;
    try {
      const saved =
        "profileId" in request
          ? await api.updateTransportProfile(request)
          : await api.createTransportProfile(request);
      if (generation !== generationRef.current) return;
      if (saved === null) {
        setNotice("Private key selection cancelled.");
        return;
      }
      setProfiles((current) => {
        const existing = current.some((profile) => profile.id === saved.id);
        return existing
          ? current.map((profile) => (profile.id === saved.id ? saved : profile))
          : [...current, saved];
      });
      setEditor(null);
      setNotice(`${saved.displayName} ${"profileId" in request ? "updated" : "created"}.`);
    } catch (reason: unknown) {
      if (generation !== generationRef.current) return;
      setError(normalizeError(reason, "Transport profile could not be saved.").message);
    } finally {
      finishOperation(generation);
    }
  };

  const inspectDeletion = async (profile: TransportProfileSummary) => {
    if (!beginOperation()) return;
    const generation = generationRef.current;
    setDeletion({ profile, impact: null, resolutions: {}, loading: true, deleting: false });
    try {
      const impact = await api.getTransportProfileDeletionImpact({ profileId: profile.id });
      if (generation !== generationRef.current) return;
      if (impact.profileId !== profile.id) throw new Error("Deletion impact profile mismatch");
      setDeletion({ profile, impact, resolutions: {}, loading: false, deleting: false });
    } catch (reason: unknown) {
      if (generation !== generationRef.current) return;
      setDeletion(null);
      setError(normalizeError(reason, "Profile deletion impact could not be loaded.").message);
    } finally {
      finishOperation(generation);
    }
  };

  const confirmDeletion = async () => {
    if (deletion === null || deletion.impact === null || !allRepositoriesResolved(deletion)) return;
    if (!beginOperation()) return;
    const generation = generationRef.current;
    const target = deletion;
    const impact = deletion.impact;
    setDeletion({ ...target, deleting: true });
    try {
      const resolutions = impact.repositories.map((repository) =>
        parseDeletionResolution(
          repository.repositoryId,
          target.resolutions[repository.repositoryId]!
        )
      );
      await api.deleteTransportProfile({ profileId: target.profile.id, resolutions });
      if (generation !== generationRef.current) return;
      setProfiles((current) => current.filter((profile) => profile.id !== target.profile.id));
      setEditor((current) => (current?.profile?.id === target.profile.id ? null : current));
      setDeletion(null);
      setNotice(`${target.profile.displayName} deleted.`);
    } catch (reason: unknown) {
      if (generation !== generationRef.current) return;
      setDeletion((current) => (current === null ? null : { ...current, deleting: false }));
      setError(normalizeError(reason, "Transport profile could not be deleted.").message);
    } finally {
      finishOperation(generation);
    }
  };

  const interactionLocked = busy || loading || refreshing || deletion !== null;

  return (
    <section className="view transport-profiles-view">
      <header className="view-header">
        <div>
          <p className="eyebrow">Git authentication</p>
          <h2>Transport identities</h2>
          <p className="muted">
            Reuse secure SSH and HTTPS transport settings without exposing credentials to plugins.
          </p>
        </div>
        <div className="button-row transport-profile-toolbar">
          <button
            type="button"
            disabled={interactionLocked}
            onClick={() => setEditor({ kind: "https", profile: null })}
          >
            New HTTPS profile
          </button>
          <button
            type="button"
            disabled={interactionLocked}
            onClick={() => setEditor({ kind: "ssh", profile: null })}
          >
            New SSH profile
          </button>
          <button
            type="button"
            disabled={interactionLocked || editor !== null}
            onClick={() => void refresh()}
          >
            {refreshing ? "Refreshing profiles…" : "Refresh profiles"}
          </button>
        </div>
      </header>

      {error === null ? null : (
        <p className="error-notice" role="alert">
          {error}
        </p>
      )}
      {notice === null ? null : <p className="success-notice">{notice}</p>}

      <article className="card system-transport-card">
        <header>
          <div>
            <h3>System Git configuration</h3>
            <p className="muted">
              Repositories without a profile continue to use the operating system Git and credential
              configuration.
            </p>
          </div>
          <span className="badge">Default</span>
        </header>
      </article>

      {editor === null ? null : (
        <TransportProfileForm
          key={`${editor.kind}:${editor.profile?.id ?? "new"}`}
          kind={editor.kind}
          profile={editor.profile}
          busy={busy || refreshing || deletion !== null}
          onSubmit={(request) => void saveProfile(request)}
          onCancel={() => setEditor(null)}
        />
      )}

      <section className="transport-profile-list" aria-labelledby="transport-profile-list-title">
        <h3 id="transport-profile-list-title">Saved transport profiles</h3>
        {loading ? <p>Loading transport profiles…</p> : null}
        {!loading && profiles.length === 0 ? <p>No transport profiles are configured.</p> : null}
        <div className="card-grid">
          {profiles.map((profile) => (
            <article className="card transport-profile-card" key={profile.id}>
              <header>
                <div>
                  <h3>{profile.displayName}</h3>
                  <p className="secondary-line">{profile.kind.toUpperCase()} transport</p>
                </div>
                <span className="badge">{profile.available ? "Available" : "Unavailable"}</span>
              </header>
              {profile.kind === "ssh" ? (
                <p>
                  Key filename: <strong>{profile.sshKeyFileName}</strong>
                </p>
              ) : (
                <p>
                  HTTPS username: <strong>{profile.httpsUsername}</strong>
                </p>
              )}
              <p className="muted">
                Used by {profile.boundRepositoryCount}{" "}
                {profile.boundRepositoryCount === 1 ? "repository" : "repositories"}.
              </p>
              <div className="button-row transport-profile-card-actions">
                <button
                  className="button-link"
                  type="button"
                  disabled={interactionLocked}
                  aria-label={`Edit ${profile.displayName}`}
                  onClick={() => setEditor({ kind: profile.kind, profile })}
                >
                  Edit
                </button>
                <button
                  className="danger-button"
                  type="button"
                  disabled={interactionLocked}
                  aria-label={`Delete ${profile.displayName}`}
                  onClick={() => void inspectDeletion(profile)}
                >
                  Delete
                </button>
              </div>
            </article>
          ))}
        </div>
      </section>

      {deletion === null ? null : (
        <div className="transport-delete-overlay" role="presentation">
          <section
            className="card transport-delete-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="transport-delete-title"
          >
            <h3 id="transport-delete-title">Delete {deletion.profile.displayName}</h3>
            {deletion.loading || deletion.impact === null ? (
              <p>Loading affected repositories…</p>
            ) : deletion.impact.repositories.length === 0 ? (
              <p>This profile is not bound to a repository.</p>
            ) : (
              <>
                <p>Choose a replacement or unbind every affected repository.</p>
                <div className="transport-delete-resolutions">
                  {deletion.impact.repositories.map((repository) => (
                    <label key={repository.repositoryId}>
                      Resolution for {repository.displayName}
                      <select
                        aria-label={`Resolution for ${repository.displayName}`}
                        disabled={deletion.deleting}
                        value={deletion.resolutions[repository.repositoryId] ?? ""}
                        onChange={(event) => {
                          const value = event.target.value;
                          setDeletion((current) =>
                            current === null
                              ? null
                              : {
                                  ...current,
                                  resolutions: {
                                    ...current.resolutions,
                                    [repository.repositoryId]: value
                                  }
                                }
                          );
                        }}
                      >
                        <option value="">Choose a resolution</option>
                        {profiles
                          .filter(
                            (candidate) =>
                              candidate.id !== deletion.profile.id &&
                              candidate.kind === repository.transportKind &&
                              candidate.available
                          )
                          .map((candidate) => (
                            <option key={candidate.id} value={`replace:${candidate.id}`}>
                              Replace with {candidate.displayName}
                            </option>
                          ))}
                        <option value="unbind:reject">
                          Unbind and restore managed configuration
                        </option>
                        <option value="unbind:keepExternal">
                          Unbind and preserve external configuration
                        </option>
                      </select>
                    </label>
                  ))}
                </div>
              </>
            )}
            <div className="button-row transport-delete-actions">
              <button
                className="danger-button"
                type="button"
                disabled={
                  deletion.loading ||
                  deletion.deleting ||
                  deletion.impact === null ||
                  !allRepositoriesResolved(deletion)
                }
                onClick={() => void confirmDeletion()}
              >
                {deletion.deleting ? "Deleting profile…" : "Confirm delete"}
              </button>
              <button
                className="button-link"
                type="button"
                disabled={deletion.loading || deletion.deleting}
                onClick={() => setDeletion(null)}
              >
                Cancel deletion
              </button>
            </div>
          </section>
        </div>
      )}
    </section>
  );
}

function allRepositoriesResolved(deletion: DeletionState): boolean {
  return (
    deletion.impact !== null &&
    deletion.impact.repositories.every(
      (repository) => (deletion.resolutions[repository.repositoryId] ?? "").length > 0
    )
  );
}

function parseDeletionResolution(
  repositoryId: string,
  value: string
): TransportProfileDeletionResolution {
  const separator = value.indexOf(":");
  const action = value.slice(0, separator);
  const target = value.slice(separator + 1);
  if (action === "replace") {
    return { repositoryId, action, replacementProfileId: target };
  }
  if (action === "unbind" && (target === "reject" || target === "keepExternal")) {
    return { repositoryId, action, driftResolution: target };
  }
  throw new Error("Transport profile deletion resolution is invalid");
}
