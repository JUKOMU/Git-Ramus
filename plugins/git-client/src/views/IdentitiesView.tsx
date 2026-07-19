import type {
  IdentityCreateRequest,
  IdentityProfile,
  IdentityUpdateRequest
} from "@git-ramus/contracts";
import { useEffect, useRef, useState, type FormEvent } from "react";
import type { GitClientApi } from "../api";
import { normalizeError } from "../api";

export type IdentitiesApi = Pick<
  GitClientApi,
  "listIdentities" | "createIdentity" | "updateIdentity" | "deleteIdentity" | "setGlobalIdentity"
>;

interface IdentitiesViewProps {
  api: IdentitiesApi;
}

interface IdentityDraft {
  displayName: string;
  userName: string;
  userEmail: string;
  gpgFormat: SigningFormat;
  signingKey: string;
  signCommits: boolean;
  signTags: boolean;
}

type SigningFormat = "none" | "openpgp" | "ssh" | "x509";

type IdentityOperation =
  | { kind: "create" }
  | { kind: "update"; profileId: string }
  | { kind: "delete"; profileId: string }
  | { kind: "setGlobal"; profileId: string };

const emptyDraft: IdentityDraft = {
  displayName: "",
  userName: "",
  userEmail: "",
  gpgFormat: "none",
  signingKey: "",
  signCommits: false,
  signTags: false
};

export function IdentitiesView({ api }: IdentitiesViewProps) {
  const [identities, setIdentities] = useState<IdentityProfile[]>([]);
  const [globalIdentityProfileId, setGlobalIdentityProfileId] = useState<string | null>(null);
  const [draft, setDraft] = useState<IdentityDraft>(emptyDraft);
  const [editingProfileId, setEditingProfileId] = useState<string | null>(null);
  const [makeGlobal, setMakeGlobal] = useState(true);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [operation, setOperation] = useState<IdentityOperation | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const generationRef = useRef(0);
  const busyRef = useRef(false);

  useEffect(() => {
    const generation = ++generationRef.current;
    busyRef.current = false;
    void Promise.resolve().then(() => {
      if (generation !== generationRef.current) return;
      setIdentities([]);
      setGlobalIdentityProfileId(null);
      setBusy(false);
      setOperation(null);
      setEditingProfileId(null);
      setDraft(emptyDraft);
      setMakeGlobal(true);
      setLoading(true);
      setError(null);
      setNotice(null);
    });
    void api
      .listIdentities()
      .then((result) => {
        if (generation !== generationRef.current) return;
        setIdentities(result.identities);
        setGlobalIdentityProfileId(result.globalIdentityProfileId);
        setMakeGlobal(result.identities.length === 0 && result.globalIdentityProfileId === null);
      })
      .catch((reason: unknown) => {
        if (generation !== generationRef.current) return;
        setError(normalizeError(reason, "Identity profiles could not be loaded.").message);
      })
      .finally(() => {
        if (generation === generationRef.current) setLoading(false);
      });
    return () => {
      if (generation === generationRef.current) generationRef.current += 1;
    };
  }, [api]);

  const editingIdentity = identities.find((identity) => identity.id === editingProfileId) ?? null;

  const beginOperation = (nextOperation: IdentityOperation) => {
    if (busyRef.current) return false;
    busyRef.current = true;
    setBusy(true);
    setOperation(nextOperation);
    setError(null);
    setNotice(null);
    return true;
  };

  const finishOperation = (generation: number) => {
    if (generation !== generationRef.current) return;
    busyRef.current = false;
    setBusy(false);
    setOperation(null);
  };

  const createIdentity = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (loading || !validDraft(draft) || !beginOperation({ kind: "create" })) return;
    const generation = generationRef.current;
    const request = identityRequest(draft);
    try {
      const created = await api.createIdentity(request);
      if (generation !== generationRef.current) return;
      setIdentities((current) => [...current, created]);
      setDraft(emptyDraft);
      setMakeGlobal(false);
      if (makeGlobal) {
        try {
          await api.setGlobalIdentity({ profileId: created.id });
          if (generation !== generationRef.current) return;
          setGlobalIdentityProfileId(created.id);
        } catch (reason: unknown) {
          if (generation !== generationRef.current) return;
          setError(normalizeError(reason, "Global identity could not be set.").message);
          return;
        }
      }
      setNotice(`${created.displayName} created.`);
    } catch (reason: unknown) {
      if (generation !== generationRef.current) return;
      setError(normalizeError(reason, "Identity profile could not be created.").message);
    } finally {
      finishOperation(generation);
    }
  };

  const updateIdentity = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (
      editingIdentity === null ||
      !validDraft(draft) ||
      !beginOperation({ kind: "update", profileId: editingIdentity.id })
    ) {
      return;
    }
    const generation = generationRef.current;
    const request: IdentityUpdateRequest = {
      profileId: editingIdentity.id,
      ...identityRequest(draft)
    };
    try {
      const updated = await api.updateIdentity(request);
      if (generation !== generationRef.current) return;
      setIdentities((current) =>
        current.map((identity) => (identity.id === updated.id ? updated : identity))
      );
      setEditingProfileId(null);
      setDraft(emptyDraft);
      setNotice(`${updated.displayName} updated.`);
    } catch (reason: unknown) {
      if (generation !== generationRef.current) return;
      setError(normalizeError(reason, "Identity profile could not be updated.").message);
    } finally {
      finishOperation(generation);
    }
  };

  const setGlobalIdentity = async (identity: IdentityProfile) => {
    if (
      identity.id === globalIdentityProfileId ||
      !beginOperation({ kind: "setGlobal", profileId: identity.id })
    ) {
      return;
    }
    const generation = generationRef.current;
    try {
      await api.setGlobalIdentity({ profileId: identity.id });
      if (generation !== generationRef.current) return;
      setGlobalIdentityProfileId(identity.id);
      setNotice(`${identity.displayName} is now the global identity.`);
    } catch (reason: unknown) {
      if (generation !== generationRef.current) return;
      setError(normalizeError(reason, "Global identity could not be changed.").message);
    } finally {
      finishOperation(generation);
    }
  };

  const deleteIdentity = async (identity: IdentityProfile) => {
    if (
      identity.id === globalIdentityProfileId ||
      !beginOperation({ kind: "delete", profileId: identity.id })
    ) {
      return;
    }
    const generation = generationRef.current;
    try {
      await api.deleteIdentity({ profileId: identity.id });
      if (generation !== generationRef.current) return;
      setIdentities((current) => current.filter((candidate) => candidate.id !== identity.id));
      if (editingProfileId === identity.id) {
        setEditingProfileId(null);
        setDraft(emptyDraft);
      }
      setNotice(`${identity.displayName} deleted.`);
    } catch (reason: unknown) {
      if (generation !== generationRef.current) return;
      setError(normalizeError(reason, "Identity profile could not be deleted.").message);
    } finally {
      finishOperation(generation);
    }
  };

  const editIdentity = (identity: IdentityProfile) => {
    if (busyRef.current) return;
    setEditingProfileId(identity.id);
    setDraft({
      displayName: identity.displayName,
      userName: identity.userName,
      userEmail: identity.userEmail,
      gpgFormat: identity.gpgFormat ?? "none",
      signingKey: identity.signingKey ?? "",
      signCommits: identity.signCommits,
      signTags: identity.signTags
    });
    setMakeGlobal(false);
    setError(null);
    setNotice(null);
  };

  const cancelEditing = () => {
    if (busyRef.current) return;
    setEditingProfileId(null);
    setDraft(emptyDraft);
    setMakeGlobal(identities.length === 0 && globalIdentityProfileId === null);
  };

  return (
    <section className="view identities-view">
      <header className="view-header">
        <div>
          <p className="eyebrow">Git authorship</p>
          <h2>Identities</h2>
          <p className="muted">
            Manage reusable Git author profiles without exposing signing keys.
          </p>
        </div>
      </header>

      {error === null ? null : (
        <p className="error-notice" role="alert">
          {error}
        </p>
      )}
      {notice === null ? null : <p className="success-notice">{notice}</p>}

      <section className="card identity-editor">
        <h3>{editingIdentity === null ? "New identity" : "Edit identity"}</h3>
        <form
          onSubmit={(event) =>
            void (editingIdentity === null ? createIdentity(event) : updateIdentity(event))
          }
        >
          <div className="form-grid identity-form-grid">
            <label>
              Profile name
              <input
                aria-label="Profile name"
                required
                maxLength={256}
                disabled={busy}
                value={draft.displayName}
                onChange={(event) =>
                  setDraft((current) => ({ ...current, displayName: event.target.value }))
                }
              />
            </label>
            <label>
              Git user name
              <input
                aria-label="Git user name"
                required
                maxLength={256}
                disabled={busy}
                value={draft.userName}
                onChange={(event) =>
                  setDraft((current) => ({ ...current, userName: event.target.value }))
                }
              />
            </label>
            <label>
              Git user email
              <input
                aria-label="Git user email"
                required
                type="email"
                maxLength={320}
                disabled={busy}
                value={draft.userEmail}
                onChange={(event) =>
                  setDraft((current) => ({ ...current, userEmail: event.target.value }))
                }
              />
            </label>
            <label>
              Signing format
              <select
                aria-label="Signing format"
                value={draft.gpgFormat}
                disabled={busy}
                onChange={(event) => {
                  const gpgFormat = event.target.value as SigningFormat;
                  setDraft((current) => ({
                    ...current,
                    gpgFormat,
                    signingKey: gpgFormat === "none" ? "" : current.signingKey
                  }));
                }}
              >
                <option value="none">None</option>
                <option value="openpgp">OpenPGP</option>
                <option value="ssh">SSH</option>
                <option value="x509">X.509</option>
              </select>
            </label>
            <label>
              Signing key
              <input
                aria-label="Signing key"
                type="password"
                autoComplete="off"
                maxLength={4 * 1024}
                required={signingEnabled(draft)}
                disabled={busy || draft.gpgFormat === "none"}
                value={draft.signingKey}
                onChange={(event) =>
                  setDraft((current) => ({ ...current, signingKey: event.target.value }))
                }
              />
            </label>
          </div>
          <div className="identity-signing-options">
            <label className="checkbox-label">
              <input
                type="checkbox"
                checked={draft.signCommits}
                disabled={busy}
                onChange={(event) =>
                  setDraft((current) => ({ ...current, signCommits: event.target.checked }))
                }
              />
              Sign commits
            </label>
            <label className="checkbox-label">
              <input
                type="checkbox"
                checked={draft.signTags}
                disabled={busy}
                onChange={(event) =>
                  setDraft((current) => ({ ...current, signTags: event.target.checked }))
                }
              />
              Sign tags
            </label>
          </div>
          {validSigningDraft(draft) ? null : (
            <p className="signing-validation">
              Choose a signing format and enter a signing key to enable signing.
            </p>
          )}
          {editingIdentity === null ? (
            <label className="checkbox-label">
              <input
                type="checkbox"
                checked={makeGlobal}
                disabled={busy}
                onChange={(event) => setMakeGlobal(event.target.checked)}
              />
              Set as global identity
            </label>
          ) : null}
          <div className="button-row identity-form-actions">
            <button type="submit" disabled={busy || loading || !validDraft(draft)}>
              {operation?.kind === "create"
                ? "Creating identity…"
                : operation?.kind === "update"
                  ? "Saving identity…"
                  : editingIdentity === null
                    ? "Create identity"
                    : "Save identity"}
            </button>
            {editingIdentity === null ? null : (
              <button className="button-link" type="button" disabled={busy} onClick={cancelEditing}>
                Cancel editing
              </button>
            )}
          </div>
        </form>
      </section>

      <section className="identity-list" aria-labelledby="identity-list-heading">
        <h3 id="identity-list-heading">Profiles</h3>
        {loading ? <p>Loading identity profiles…</p> : null}
        {!loading && identities.length === 0 ? <p>No identity profiles are configured.</p> : null}
        <div className="card-grid">
          {identities.map((identity) => {
            const isGlobal = identity.id === globalIdentityProfileId;
            const settingGlobal =
              operation?.kind === "setGlobal" && operation.profileId === identity.id;
            const deleting = operation?.kind === "delete" && operation.profileId === identity.id;
            return (
              <article className="card" key={identity.id}>
                <header>
                  <div>
                    <h3>{identity.displayName}</h3>
                    <p className="secondary-line">
                      {identity.userName} · {identity.userEmail}
                    </p>
                  </div>
                  {isGlobal ? <span className="badge">Global identity</span> : null}
                </header>
                <p className="muted">
                  {identity.signCommits ? "Commit signing enabled" : "Commit signing disabled"}
                </p>
                {isGlobal ? (
                  <p className="muted">
                    Choose another global identity before deleting this profile.
                  </p>
                ) : null}
                <div className="button-row identity-card-actions">
                  <button
                    className="button-link"
                    type="button"
                    disabled={busy}
                    aria-label={`Edit ${identity.displayName}`}
                    onClick={() => editIdentity(identity)}
                  >
                    Edit
                  </button>
                  {isGlobal ? null : (
                    <button
                      type="button"
                      disabled={busy}
                      aria-label={
                        settingGlobal
                          ? `Setting ${identity.displayName} as global…`
                          : `Set ${identity.displayName} as global identity`
                      }
                      onClick={() => void setGlobalIdentity(identity)}
                    >
                      {settingGlobal ? "Setting global…" : "Set global"}
                    </button>
                  )}
                  <button
                    className="danger-button"
                    type="button"
                    disabled={busy || isGlobal}
                    title={
                      isGlobal
                        ? "Choose another global identity before deleting this profile"
                        : undefined
                    }
                    aria-label={
                      deleting
                        ? `Deleting ${identity.displayName}…`
                        : `Delete ${identity.displayName}`
                    }
                    onClick={() => void deleteIdentity(identity)}
                  >
                    {deleting ? "Deleting…" : "Delete"}
                  </button>
                </div>
              </article>
            );
          })}
        </div>
      </section>
    </section>
  );
}

function validDraft(draft: IdentityDraft) {
  return (
    draft.displayName.trim().length > 0 &&
    draft.userName.trim().length > 0 &&
    draft.userEmail.trim().length > 0 &&
    validSigningDraft(draft)
  );
}

function signingEnabled(draft: IdentityDraft) {
  return draft.signCommits || draft.signTags;
}

function validSigningDraft(draft: IdentityDraft) {
  return (
    !signingEnabled(draft) || (draft.gpgFormat !== "none" && draft.signingKey.trim().length > 0)
  );
}

function identityRequest(draft: IdentityDraft): IdentityCreateRequest {
  const gpgFormat = draft.gpgFormat === "none" ? null : draft.gpgFormat;
  return {
    displayName: draft.displayName.trim(),
    userName: draft.userName.trim(),
    userEmail: draft.userEmail.trim(),
    gpgFormat,
    signingKey: gpgFormat === null ? null : draft.signingKey.trim() || null,
    signCommits: draft.signCommits,
    signTags: draft.signTags
  };
}
