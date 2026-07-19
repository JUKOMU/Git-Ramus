import type {
  GitContextRequest,
  Overview,
  PersistedRepositorySnapshot,
  ProjectListResponse,
  Repository,
  RepositoryRequest,
  RepositoryScanRecord,
  WorkspaceListResponse
} from "@git-ramus/contracts";
import { useEffect, useMemo, useState } from "react";
import type { GitClientApi } from "../api";
import { normalizeError } from "../api";

export type OverviewApi = Pick<
  GitClientApi,
  "listProjects" | "listWorkspaces" | "getOverview" | "getRepositorySnapshot"
>;

interface OverviewViewProps {
  api: OverviewApi;
  onOpenRepository(repository: Repository, context: GitContextRequest): void;
}

interface ContextOption {
  value: string;
  label: string;
  context: GitContextRequest;
}

interface LoadedRepository {
  repository: Repository;
  snapshot: PersistedRepositorySnapshot | null;
  error: string | null;
}

const SNAPSHOT_BATCH_SIZE = 4;

export function OverviewView({ api, onOpenRepository }: OverviewViewProps) {
  const [contexts, setContexts] = useState<ContextOption[]>([]);
  const [selectedContext, setSelectedContext] = useState("");
  const [overview, setOverview] = useState<Overview | null>(null);
  const [repositories, setRepositories] = useState<LoadedRepository[]>([]);
  const [loadingOverview, setLoadingOverview] = useState(true);
  const [loadingRepositories, setLoadingRepositories] = useState(false);
  const [branchFilter, setBranchFilter] = useState("all");
  const [statusFilter, setStatusFilter] = useState("all");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    void Promise.all([api.listProjects(), api.listWorkspaces()])
      .then(([projectResult, workspaceResult]) => {
        if (!active) return;
        const options = contextOptions(projectResult, workspaceResult);
        setContexts(options);
        setSelectedContext((current) => current || options[0]?.value || "");
        if (options.length === 0) {
          setLoadingOverview(false);
        }
      })
      .catch((reason: unknown) => {
        if (!active) return;
        setError(normalizeError(reason, "Overview contexts could not be loaded.").message);
        setLoadingOverview(false);
      });
    return () => {
      active = false;
    };
  }, [api]);

  useEffect(() => {
    const option = contexts.find((candidate) => candidate.value === selectedContext);
    if (option === undefined) return;
    let active = true;
    void Promise.resolve()
      .then(() => {
        if (!active) return null;
        setLoadingOverview(true);
        setLoadingRepositories(false);
        setOverview(null);
        setRepositories([]);
        setBranchFilter("all");
        setError(null);
        return api.getOverview(option.context);
      })
      .then(async (loadedOverview) => {
        if (!active || loadedOverview === null) return;
        setOverview(loadedOverview);
        setLoadingOverview(false);
        setLoadingRepositories(loadedOverview.repositories.length > 0);

        for (
          let index = 0;
          index < loadedOverview.repositories.length;
          index += SNAPSHOT_BATCH_SIZE
        ) {
          const batch = loadedOverview.repositories.slice(index, index + SNAPSHOT_BATCH_SIZE);
          await Promise.all(
            batch.map(async (entry) => {
              const request: RepositoryRequest = {
                ...option.context,
                repositoryId: entry.repository.id
              };
              try {
                const record = await api.getRepositorySnapshot(request);
                if (!active) return;
                setRepositories((current) => appendRepository(current, fromRecord(record)));
              } catch (reason: unknown) {
                if (!active) return;
                const message = normalizeError(
                  reason,
                  `Snapshot for ${entry.repository.displayName} could not be loaded.`
                ).message;
                setRepositories((current) =>
                  appendRepository(current, {
                    repository: entry.repository,
                    snapshot: entry.snapshot,
                    error: message
                  })
                );
              }
            })
          );
        }
        if (active) setLoadingRepositories(false);
      })
      .catch((reason: unknown) => {
        if (!active) return;
        setError(normalizeError(reason, "Overview could not be loaded.").message);
        setLoadingOverview(false);
      });

    return () => {
      active = false;
    };
  }, [api, contexts, selectedContext]);

  const currentContext =
    contexts.find((candidate) => candidate.value === selectedContext)?.context ?? null;
  const filteredRepositories = useMemo(
    () =>
      repositories.filter(({ snapshot }) => {
        const matchesBranch = branchFilter === "all" || snapshot?.branch === branchFilter;
        const matchesStatus =
          statusFilter === "all" ||
          (statusFilter === "dirty" && snapshot?.dirty === true) ||
          (statusFilter === "clean" && snapshot?.dirty === false) ||
          (statusFilter === "staged" && (snapshot?.stagedCount ?? 0) > 0) ||
          (statusFilter === "conflicted" && (snapshot?.conflictedCount ?? 0) > 0);
        return matchesBranch && matchesStatus;
      }),
    [branchFilter, repositories, statusFilter]
  );

  return (
    <section className="view overview-view">
      <header className="view-header">
        <div>
          <p className="eyebrow">Repositories</p>
          <h2>Overview</h2>
        </div>
        <div className="filters">
          <label>
            Context
            <select
              aria-label="Context filter"
              value={selectedContext}
              onChange={(event) => setSelectedContext(event.target.value)}
            >
              {contexts.map((context) => (
                <option key={context.value} value={context.value}>
                  {context.label}
                </option>
              ))}
            </select>
          </label>
          <label>
            Branch
            <select
              aria-label="Branch filter"
              value={branchFilter}
              onChange={(event) => setBranchFilter(event.target.value)}
            >
              <option value="all">All branches</option>
              {overview?.branches.map((branch) => (
                <option key={branch} value={branch}>
                  {branch}
                </option>
              ))}
            </select>
          </label>
          <label>
            Status
            <select
              aria-label="Status filter"
              value={statusFilter}
              onChange={(event) => setStatusFilter(event.target.value)}
            >
              <option value="all">All statuses</option>
              <option value="dirty">Dirty</option>
              <option value="clean">Clean</option>
              <option value="staged">Staged</option>
              <option value="conflicted">Conflicted</option>
            </select>
          </label>
        </div>
      </header>

      {loadingOverview ? <p>Loading overview…</p> : null}
      {error ? <p className="error-notice">{error}</p> : null}
      {!loadingOverview && contexts.length === 0 ? (
        <p>No projects or workspaces are available yet.</p>
      ) : null}
      {loadingRepositories ? (
        <p>
          Loading repositories {repositories.length}/{overview?.repositories.length ?? 0}…
        </p>
      ) : null}
      {!loadingOverview && overview !== null && filteredRepositories.length === 0 ? (
        <p>No repositories match the selected filters.</p>
      ) : null}
      {filteredRepositories.length > 0 ? (
        <div className="table-scroll">
          <table>
            <thead>
              <tr>
                <th>Repository</th>
                <th>Branch</th>
                <th>Status</th>
                <th>Changes</th>
                <th aria-label="Repository actions" />
              </tr>
            </thead>
            <tbody>
              {filteredRepositories.map(({ repository, snapshot, error: rowError }) => (
                <tr key={repository.id}>
                  <td>
                    <strong>{repository.displayName}</strong>
                    <span className="secondary-line">{repository.canonicalPath}</span>
                  </td>
                  <td>{snapshot?.branch ?? "Detached / unknown"}</td>
                  <td>{rowError ?? (snapshot?.dirty ? "Dirty" : "Clean")}</td>
                  <td>
                    {(snapshot?.stagedCount ?? 0) +
                      (snapshot?.unstagedCount ?? 0) +
                      (snapshot?.untrackedCount ?? 0) +
                      (snapshot?.conflictedCount ?? 0)}
                  </td>
                  <td>
                    <button
                      type="button"
                      disabled={currentContext === null}
                      onClick={() => {
                        if (currentContext !== null) onOpenRepository(repository, currentContext);
                      }}
                    >
                      Open
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : null}
    </section>
  );
}

function contextOptions(
  projects: ProjectListResponse,
  workspaces: WorkspaceListResponse
): ContextOption[] {
  return [
    ...projects.projects.map((project) => ({
      value: `project:${project.id}`,
      label: `Project · ${project.name}`,
      context: { projectId: project.id }
    })),
    ...workspaces.workspaces.map((workspace) => ({
      value: `workspace:${workspace.id}`,
      label: `Workspace · ${workspace.name}`,
      context: { workspaceId: workspace.id }
    }))
  ];
}

function fromRecord(record: RepositoryScanRecord): LoadedRepository {
  return {
    repository: record.repository,
    snapshot: record.snapshot,
    error: record.error
  };
}

function appendRepository(
  repositories: LoadedRepository[],
  repository: LoadedRepository
): LoadedRepository[] {
  return [
    ...repositories.filter((current) => current.repository.id !== repository.repository.id),
    repository
  ];
}
