import {
  cloneRequestSchema,
  type CloneIntentSummary,
  type CloneResult,
  type ErrorEnvelope,
  type Project,
  type TransportKind,
  type TransportProfileSummary
} from "@git-ramus/contracts";
import { useEffect, useMemo, useRef, useState } from "react";
import type { GitClientApi } from "../api";
import { normalizeError } from "../api";

export type CloneApi = Pick<
  GitClientApi,
  "listProjects" | "listTransportProfiles" | "getCloneIntent" | "cloneRepository"
>;

interface CloneViewProps {
  api: CloneApi;
  intentId: string | null;
  onCloned(result: CloneResult): void;
}

type ProjectTargetMode = "existing" | "new";

export function CloneView({ api, intentId, onCloned }: CloneViewProps) {
  const [intent, setIntent] = useState<CloneIntentSummary | null>(null);
  const [projects, setProjects] = useState<Project[]>([]);
  const [profiles, setProfiles] = useState<TransportProfileSummary[]>([]);
  const [remoteUrl, setRemoteUrl] = useState("");
  const [folderName, setFolderName] = useState("");
  const [transportKind, setTransportKind] = useState<TransportKind>("https");
  const [profileId, setProfileId] = useState("");
  const [projectMode, setProjectMode] = useState<ProjectTargetMode>("existing");
  const [projectId, setProjectId] = useState("");
  const [newProjectName, setNewProjectName] = useState("");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<ErrorEnvelope | null>(null);
  const [errorPhase, setErrorPhase] = useState<"load" | "clone" | null>(null);
  const [partialError, setPartialError] = useState<ErrorEnvelope | null>(null);
  const [partialTerminal, setPartialTerminal] = useState(false);
  const [reloadKey, setReloadKey] = useState(0);
  const lifecycleRef = useRef(0);
  const operationRef = useRef(0);
  const busyRef = useRef(false);
  const abortRef = useRef<AbortController | null>(null);

  useEffect(() => {
    const lifecycle = ++lifecycleRef.current;
    abortRef.current?.abort();
    abortRef.current = null;
    busyRef.current = false;
    void Promise.resolve().then(() => {
      if (lifecycle !== lifecycleRef.current) return;
      setIntent(null);
      setProjects([]);
      setProfiles([]);
      setRemoteUrl("");
      setFolderName("");
      setTransportKind("https");
      setProfileId("");
      setProjectMode("existing");
      setProjectId("");
      setNewProjectName("");
      setLoading(true);
      setBusy(false);
      setNotice(null);
      setError(null);
      setErrorPhase(null);
      setPartialError(null);
      setPartialTerminal(false);
    });
    const intentRequest =
      intentId === null ? Promise.resolve(null) : api.getCloneIntent({ intentId });
    void Promise.all([api.listProjects(), api.listTransportProfiles(), intentRequest])
      .then(([projectResult, profileResult, loadedIntent]) => {
        if (lifecycle !== lifecycleRef.current) return;
        setProjects(projectResult.projects);
        setProfiles(profileResult.items);
        setIntent(loadedIntent);
        const firstProject = projectResult.projects[0];
        setProjectMode(firstProject === undefined ? "new" : "existing");
        setProjectId(firstProject?.id ?? "");
        if (loadedIntent !== null) {
          setFolderName(loadedIntent.repository.name);
          setTransportKind(loadedIntent.availableTransports[0] ?? "https");
        }
      })
      .catch((reason: unknown) => {
        if (lifecycle !== lifecycleRef.current) return;
        setError(normalizeError(reason, "Clone information could not be loaded."));
        setErrorPhase("load");
      })
      .finally(() => {
        if (lifecycle === lifecycleRef.current) setLoading(false);
      });
    return () => {
      if (lifecycle === lifecycleRef.current) lifecycleRef.current += 1;
      operationRef.current += 1;
      busyRef.current = false;
      abortRef.current?.abort();
      abortRef.current = null;
    };
  }, [api, intentId, reloadKey]);

  const allowedTransports = intent?.availableTransports ?? (["https", "ssh"] as const);
  const compatibleProfiles = useMemo(
    () => profiles.filter((profile) => profile.kind === transportKind && profile.available),
    [profiles, transportKind]
  );
  const requestDraft = {
    source:
      intentId === null
        ? { kind: "manual" as const, remoteUrl }
        : { kind: "intent" as const, intentId },
    transportKind,
    profileId: profileId.length === 0 ? null : profileId,
    folderName,
    projectTarget:
      projectMode === "existing"
        ? { kind: "existing" as const, projectId }
        : { kind: "new" as const, name: newProjectName },
    operationId: "00000000-0000-4000-8000-000000000000"
  };
  const validRequest = cloneRequestSchema.safeParse(requestDraft).success;
  const folderValid = safeFolderName(folderName);

  const clone = async () => {
    if (loading || partialTerminal || !validRequest || busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setNotice(null);
    setError(null);
    setErrorPhase(null);
    setPartialError(null);
    const lifecycle = lifecycleRef.current;
    const operation = ++operationRef.current;
    const controller = new AbortController();
    abortRef.current = controller;
    const request = cloneRequestSchema.parse({
      ...requestDraft,
      operationId: crypto.randomUUID()
    });
    try {
      const result = await api.cloneRepository(request, controller.signal);
      if (lifecycle !== lifecycleRef.current || operation !== operationRef.current) return;
      if (result === null) {
        setNotice("Clone cancelled.");
      } else if (result.status === "partial") {
        setNotice("Repository content was cloned, but setup is incomplete.");
        setPartialError(result.job.error);
        setPartialTerminal(true);
      } else {
        setNotice("Repository cloned.");
        onCloned(result);
      }
    } catch (reason: unknown) {
      if (lifecycle !== lifecycleRef.current || operation !== operationRef.current) return;
      if (isAbortError(reason)) setNotice("Clone cancelled.");
      else {
        setError(normalizeError(reason, "Repository could not be cloned."));
        setErrorPhase("clone");
      }
    } finally {
      if (lifecycle === lifecycleRef.current && operation === operationRef.current) {
        busyRef.current = false;
        abortRef.current = null;
        setBusy(false);
      }
    }
  };

  return (
    <section className="view clone-view">
      <header className="view-header">
        <div>
          <p className="eyebrow">Repository onboarding</p>
          <h2>Clone repository</h2>
          <p className="muted">
            Git-Ramus validates the source and lets the trusted host choose the destination folder.
          </p>
        </div>
      </header>

      {error === null ? null : (
        <CloneErrorNotice
          error={error}
          busy={busy}
          onRetry={
            errorPhase === "load"
              ? () => setReloadKey((current) => current + 1)
              : () => void clone()
          }
        />
      )}
      {partialError === null ? null : <CloneErrorNotice error={partialError} />}
      {notice === null ? null : <p className="success-notice">{notice}</p>}
      {loading ? <p>Loading Clone options…</p> : null}

      <section className="card clone-source-card">
        <h3>{intentId === null ? "Manual Git remote" : "Provider repository"}</h3>
        {intentId === null ? (
          <label>
            Git remote URL
            <input
              aria-label="Git remote URL"
              maxLength={4096}
              disabled={busy || partialTerminal}
              value={remoteUrl}
              onChange={(event) => setRemoteUrl(event.target.value)}
            />
          </label>
        ) : intent === null ? null : (
          <div className="clone-provider-summary">
            <strong>{intent.repository.fullName}</strong>
            <span className="muted">
              {intent.repository.providerKind.toUpperCase()} · {intent.repository.visibility}
            </span>
          </div>
        )}
      </section>

      <section className="card clone-options-card">
        <h3>Clone options</h3>
        <fieldset className="clone-transport-options" disabled={busy || loading || partialTerminal}>
          <legend>Transport</legend>
          {(["https", "ssh"] as const).map((kind) => (
            <label className="checkbox-label" key={kind}>
              <input
                type="radio"
                name="clone-transport"
                aria-label={kind.toUpperCase()}
                checked={transportKind === kind}
                disabled={!allowedTransports.includes(kind)}
                onChange={() => {
                  setTransportKind(kind);
                  setProfileId("");
                }}
              />
              {kind.toUpperCase()}
            </label>
          ))}
        </fieldset>
        <div className="form-grid clone-form-grid">
          <label>
            Transport profile
            <select
              aria-label="Transport profile"
              disabled={busy || loading || partialTerminal}
              value={profileId}
              onChange={(event) => setProfileId(event.target.value)}
            >
              <option value="">System Git configuration</option>
              {compatibleProfiles.map((profile) => (
                <option key={profile.id} value={profile.id}>
                  {profile.displayName}
                </option>
              ))}
            </select>
          </label>
          <label>
            Folder name
            <input
              aria-label="Folder name"
              maxLength={255}
              disabled={busy || partialTerminal}
              value={folderName}
              onChange={(event) => setFolderName(event.target.value)}
            />
          </label>
        </div>
        {folderName.length === 0 || folderValid ? null : (
          <p className="signing-validation">Choose a safe single folder name.</p>
        )}
      </section>

      <section className="card clone-project-card">
        <h3>Project</h3>
        <div className="clone-project-modes">
          <label className="checkbox-label">
            <input
              type="radio"
              name="clone-project-target"
              checked={projectMode === "existing"}
              disabled={busy || partialTerminal || projects.length === 0}
              onChange={() => setProjectMode("existing")}
            />
            Existing project
          </label>
          <label className="checkbox-label">
            <input
              type="radio"
              name="clone-project-target"
              checked={projectMode === "new"}
              disabled={busy || partialTerminal}
              onChange={() => setProjectMode("new")}
            />
            New project
          </label>
        </div>
        {projectMode === "existing" ? (
          <label>
            Project
            <select
              aria-label="Project"
              disabled={busy || partialTerminal}
              value={projectId}
              onChange={(event) => setProjectId(event.target.value)}
            >
              {projects.map((project) => (
                <option key={project.id} value={project.id}>
                  {project.name}
                </option>
              ))}
            </select>
          </label>
        ) : (
          <label>
            New project name
            <input
              aria-label="New project name"
              maxLength={128}
              disabled={busy || partialTerminal}
              value={newProjectName}
              onChange={(event) => setNewProjectName(event.target.value)}
            />
          </label>
        )}
      </section>

      <div className="button-row clone-actions">
        <button
          type="button"
          disabled={busy || loading || partialTerminal || !validRequest}
          onClick={() => void clone()}
        >
          {busy ? "Cloning repository…" : "Clone repository"}
        </button>
        {busy ? (
          <button type="button" onClick={() => abortRef.current?.abort()}>
            Cancel clone
          </button>
        ) : null}
      </div>
    </section>
  );
}

function safeFolderName(value: string): boolean {
  return (
    value.length > 0 &&
    value.length <= 255 &&
    value !== "." &&
    value !== ".." &&
    !value.includes("/") &&
    !value.includes("\\") &&
    !Array.from(value).some((character) => {
      const code = character.charCodeAt(0);
      return code < 0x20 || code === 0x7f;
    })
  );
}

function isAbortError(reason: unknown): boolean {
  return reason instanceof DOMException && reason.name === "AbortError";
}

function CloneErrorNotice({
  error,
  busy = false,
  onRetry
}: {
  error: ErrorEnvelope;
  busy?: boolean;
  onRetry?(): void;
}) {
  return (
    <div className="error-notice" role="alert">
      <p>{error.message}</p>
      {error.recoveryActions.map((action) => (
        <button
          key={action.id}
          type="button"
          disabled={busy || action.kind !== "retry" || onRetry === undefined}
          title={
            action.kind === "retry" && onRetry !== undefined
              ? undefined
              : "Complete this action in the Git-Ramus host"
          }
          onClick={action.kind === "retry" ? onRetry : undefined}
        >
          {action.label}
        </button>
      ))}
    </div>
  );
}
