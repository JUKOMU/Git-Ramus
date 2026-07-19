import type { ErrorEnvelope, Project, Workspace } from "@git-ramus/contracts";
import { useCallback, useEffect, useRef, useState } from "react";
import type { GitClientApi } from "../api";
import { normalizeError } from "../api";

export type WorkspacesApi = Pick<
  GitClientApi,
  | "listProjects"
  | "listWorkspaces"
  | "getWorkspaceMembership"
  | "createWorkspace"
  | "updateWorkspaceMembership"
  | "deleteWorkspace"
>;

interface WorkspacesViewProps {
  api: WorkspacesApi;
}

interface WorkspaceFailure {
  error: ErrorEnvelope;
  operation: "load" | "update" | "delete";
  projectIds: string[] | null;
}

export function WorkspacesView({ api }: WorkspacesViewProps) {
  const [projects, setProjects] = useState<Project[]>([]);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [memberships, setMemberships] = useState<Record<string, string[] | undefined>>({});
  const [failures, setFailures] = useState<Record<string, WorkspaceFailure | undefined>>({});
  const [pending, setPending] = useState<Set<string>>(new Set());
  const [name, setName] = useState("");
  const [globalError, setGlobalError] = useState<string | null>(null);
  const workspaceGenerations = useRef(new Map<string, number>());
  const pendingTokens = useRef(new Map<string, number>());
  const mounted = useRef(false);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const beginWorkspaceOperation = useCallback(
    (workspaceId: string, supersede: boolean): number | null => {
      if (!mounted.current || (!supersede && pendingTokens.current.has(workspaceId))) {
        return null;
      }
      const token = (workspaceGenerations.current.get(workspaceId) ?? 0) + 1;
      workspaceGenerations.current.set(workspaceId, token);
      pendingTokens.current.set(workspaceId, token);
      setPending((current) => new Set(current).add(workspaceId));
      return token;
    },
    []
  );

  const isCurrentWorkspaceOperation = useCallback(
    (workspaceId: string, token: number) =>
      mounted.current && workspaceGenerations.current.get(workspaceId) === token,
    []
  );

  const finishWorkspaceOperation = useCallback(
    (workspaceId: string, token: number) => {
      if (!isCurrentWorkspaceOperation(workspaceId, token)) return;
      pendingTokens.current.delete(workspaceId);
      setPending((current) => {
        const next = new Set(current);
        next.delete(workspaceId);
        return next;
      });
    },
    [isCurrentWorkspaceOperation]
  );

  const clearWorkspaceState = (workspaceId: string) => {
    pendingTokens.current.delete(workspaceId);
    setMemberships((current) => omitKey(current, workspaceId));
    setFailures((current) => omitKey(current, workspaceId));
    setPending((current) => {
      const next = new Set(current);
      next.delete(workspaceId);
      return next;
    });
  };

  const loadMembership = useCallback(
    async (workspaceId: string, supersede = false) => {
      const token = beginWorkspaceOperation(workspaceId, supersede);
      if (token === null) return;
      try {
        const projectIds = await api.getWorkspaceMembership({ workspaceId });
        if (!isCurrentWorkspaceOperation(workspaceId, token)) return;
        setMemberships((current) => ({ ...current, [workspaceId]: projectIds }));
        setFailures((current) => omitKey(current, workspaceId));
      } catch (reason: unknown) {
        if (!isCurrentWorkspaceOperation(workspaceId, token)) return;
        const error = normalizeError(reason, "Membership could not be loaded.");
        setFailures((current) => ({
          ...current,
          [workspaceId]: { error, operation: "load", projectIds: null }
        }));
      } finally {
        finishWorkspaceOperation(workspaceId, token);
      }
    },
    [api, beginWorkspaceOperation, finishWorkspaceOperation, isCurrentWorkspaceOperation]
  );

  useEffect(() => {
    let active = true;
    void Promise.all([api.listProjects(), api.listWorkspaces()])
      .then(([projectResult, workspaceResult]) => {
        if (!active) return;
        setProjects(projectResult.projects);
        setWorkspaces(workspaceResult.workspaces);
        for (const workspace of workspaceResult.workspaces) {
          void loadMembership(workspace.id, true);
        }
      })
      .catch((reason: unknown) => {
        if (active)
          setGlobalError(normalizeError(reason, "Workspaces could not be loaded.").message);
      });
    return () => {
      active = false;
    };
  }, [api, loadMembership]);

  const createWorkspace = async () => {
    const trimmedName = name.trim();
    if (!trimmedName) return;
    setGlobalError(null);
    try {
      const workspace = await api.createWorkspace({ name: trimmedName });
      workspaceGenerations.current.set(
        workspace.id,
        (workspaceGenerations.current.get(workspace.id) ?? 0) + 1
      );
      clearWorkspaceState(workspace.id);
      setWorkspaces((current) => [...current, workspace]);
      setMemberships((current) => ({ ...current, [workspace.id]: [] }));
      setName("");
    } catch (reason: unknown) {
      setGlobalError(normalizeError(reason, "Workspace could not be created.").message);
    }
  };

  const updateMembership = async (workspaceId: string, projectIds: string[]) => {
    const token = beginWorkspaceOperation(workspaceId, false);
    if (token === null) return;
    try {
      const confirmed = await api.updateWorkspaceMembership({ workspaceId, projectIds });
      if (!isCurrentWorkspaceOperation(workspaceId, token)) return;
      setMemberships((current) => ({ ...current, [workspaceId]: confirmed }));
      setFailures((current) => omitKey(current, workspaceId));
    } catch (reason: unknown) {
      if (!isCurrentWorkspaceOperation(workspaceId, token)) return;
      const error = normalizeError(reason, "Membership could not be updated.");
      setFailures((current) => ({
        ...current,
        [workspaceId]: { error, operation: "update", projectIds }
      }));
    } finally {
      finishWorkspaceOperation(workspaceId, token);
    }
  };

  const deleteWorkspace = async (workspaceId: string) => {
    const token = beginWorkspaceOperation(workspaceId, false);
    if (token === null) return;
    try {
      await api.deleteWorkspace({ workspaceId });
      if (!isCurrentWorkspaceOperation(workspaceId, token)) return;
      workspaceGenerations.current.set(workspaceId, token + 1);
      clearWorkspaceState(workspaceId);
      setWorkspaces((current) => current.filter((workspace) => workspace.id !== workspaceId));
    } catch (reason: unknown) {
      if (!isCurrentWorkspaceOperation(workspaceId, token)) return;
      const error = normalizeError(reason, "Workspace could not be deleted.");
      setFailures((current) => ({
        ...current,
        [workspaceId]: { error, operation: "delete", projectIds: null }
      }));
    } finally {
      finishWorkspaceOperation(workspaceId, token);
    }
  };

  return (
    <section className="view workspaces-view">
      <header className="view-header">
        <div>
          <p className="eyebrow">Virtual collections</p>
          <h2>Workspaces</h2>
        </div>
        <div className="inline-form">
          <label>
            Workspace name
            <input value={name} onChange={(event) => setName(event.target.value)} />
          </label>
          <button type="button" disabled={!name.trim()} onClick={() => void createWorkspace()}>
            Create workspace
          </button>
        </div>
      </header>
      {globalError ? <p className="error-notice">{globalError}</p> : null}
      {workspaces.length === 0 ? <p>No workspaces are available.</p> : null}
      <div className="card-grid">
        {workspaces.map((workspace) => {
          const membership = memberships[workspace.id];
          const failure = failures[workspace.id];
          const isPending = pending.has(workspace.id);
          const retryFailure = () => {
            if (isPending) return;
            if (failure?.operation === "update" && failure.projectIds !== null) {
              void updateMembership(workspace.id, failure.projectIds);
            } else if (failure?.operation === "delete") {
              void deleteWorkspace(workspace.id);
            } else {
              void loadMembership(workspace.id);
            }
          };
          return (
            <article className="card" key={workspace.id}>
              <header>
                <h3>{workspace.name}</h3>
                <button
                  className="danger-button"
                  type="button"
                  disabled={isPending}
                  onClick={() => void deleteWorkspace(workspace.id)}
                >
                  Delete
                </button>
              </header>
              {failure === undefined ? null : (
                <div className="error-notice" role="alert">
                  <p>{failure.error.message}</p>
                  {(failure.error.recoveryActions.length > 0
                    ? failure.error.recoveryActions
                    : failure.error.retryable
                      ? [{ id: "retry", label: "Try again", kind: "retry" as const }]
                      : []
                  ).map((action) => (
                    <button
                      key={action.id}
                      type="button"
                      disabled={isPending || action.kind !== "retry"}
                      title={
                        action.kind === "retry"
                          ? undefined
                          : "Complete this action in the Git-Ramus host"
                      }
                      onClick={action.kind === "retry" ? retryFailure : undefined}
                    >
                      {action.label}
                    </button>
                  ))}
                </div>
              )}
              {membership === undefined && failure === undefined ? (
                <p>Loading membership…</p>
              ) : null}
              {membership === undefined ? null : (
                <ul className="membership-list">
                  {projects.map((project) => {
                    const isMember = membership.includes(project.id);
                    const nextProjectIds = isMember
                      ? membership.filter((projectId) => projectId !== project.id)
                      : [...membership, project.id];
                    const label = `${isMember ? "Remove" : "Add"} ${project.name} ${
                      isMember ? "from" : "to"
                    } ${workspace.name}`;
                    return (
                      <li key={project.id}>
                        <span>{project.name}</span>
                        <button
                          type="button"
                          aria-label={label}
                          disabled={isPending}
                          onClick={() => void updateMembership(workspace.id, nextProjectIds)}
                        >
                          {isMember ? "Remove" : "Add"}
                        </button>
                      </li>
                    );
                  })}
                </ul>
              )}
            </article>
          );
        })}
      </div>
    </section>
  );
}

function omitKey<T>(record: Record<string, T | undefined>, key: string) {
  const next = { ...record };
  delete next[key];
  return next;
}
