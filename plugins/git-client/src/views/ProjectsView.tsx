import type { GitContextRequest, Repository } from "@git-ramus/contracts";
import { useEffect, useRef, useState } from "react";
import type { GitClientApi } from "../api";
import { normalizeError } from "../api";

export type ProjectsApi = Pick<
  GitClientApi,
  "listProjects" | "createProject" | "updateProjectScanRules" | "scanProject"
>;

interface ProjectsViewProps {
  api: ProjectsApi;
  onOpenRepository(repository: Repository, context: GitContextRequest): void;
}

interface ProjectDraft {
  scanDepth: string;
  exclusions: string;
}

export function ProjectsView({ api, onOpenRepository }: ProjectsViewProps) {
  const [projects, setProjects] = useState<
    Awaited<ReturnType<ProjectsApi["listProjects"]>>["projects"]
  >([]);
  const [drafts, setDrafts] = useState<Record<string, ProjectDraft>>({});
  const [scanResults, setScanResults] = useState<
    Record<string, Awaited<ReturnType<ProjectsApi["scanProject"]>> | undefined>
  >({});
  const [busyProjectIds, setBusyProjectIds] = useState<Set<string>>(new Set());
  const [rootSelectionBusy, setRootSelectionBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const busyProjectIdsRef = useRef(new Set<string>());
  const rootSelectionBusyRef = useRef(false);

  const beginProjectOperation = (projectId: string) => {
    if (busyProjectIdsRef.current.has(projectId)) return false;
    const next = new Set(busyProjectIdsRef.current).add(projectId);
    busyProjectIdsRef.current = next;
    setBusyProjectIds(next);
    return true;
  };

  const finishProjectOperation = (projectId: string) => {
    const next = new Set(busyProjectIdsRef.current);
    next.delete(projectId);
    busyProjectIdsRef.current = next;
    setBusyProjectIds(next);
  };

  useEffect(() => {
    let active = true;
    void api
      .listProjects()
      .then(({ projects: loadedProjects }) => {
        if (!active) return;
        setProjects(loadedProjects);
        setDrafts(
          Object.fromEntries(loadedProjects.map((project) => [project.id, draft(project)]))
        );
      })
      .catch((reason: unknown) => {
        if (active) setError(normalizeError(reason, "Projects could not be loaded.").message);
      });
    return () => {
      active = false;
    };
  }, [api]);

  const chooseRoot = async () => {
    if (rootSelectionBusyRef.current) return;
    rootSelectionBusyRef.current = true;
    setRootSelectionBusy(true);
    try {
      const created = await api.createProject();
      if (created === null) return;
      const { projects: loadedProjects } = await api.listProjects();
      setProjects(loadedProjects);
      setDrafts(Object.fromEntries(loadedProjects.map((project) => [project.id, draft(project)])));
      setError(null);
      setNotice(`Project ${created.name} created.`);
    } catch (reason: unknown) {
      setNotice(null);
      setError(normalizeError(reason, "Project root could not be selected.").message);
    } finally {
      rootSelectionBusyRef.current = false;
      setRootSelectionBusy(false);
    }
  };

  const saveScanRules = async (projectId: string) => {
    const currentDraft = drafts[projectId];
    if (currentDraft === undefined || !beginProjectOperation(projectId)) return;
    setNotice(null);
    setError(null);
    try {
      const updated = await api.updateProjectScanRules({
        projectId,
        scanDepth: Number(currentDraft.scanDepth),
        excludePatterns: currentDraft.exclusions
          .split(/\r?\n/u)
          .map((pattern) => pattern.trim())
          .filter(Boolean)
      });
      setProjects((current) =>
        current.map((project) => (project.id === updated.id ? updated : project))
      );
      setDrafts((current) => ({ ...current, [updated.id]: draft(updated) }));
      setNotice(`Scan rules saved for ${updated.name}.`);
    } catch (reason: unknown) {
      setError(normalizeError(reason, "Scan rules could not be saved.").message);
    } finally {
      finishProjectOperation(projectId);
    }
  };

  const scan = async (projectId: string) => {
    if (!beginProjectOperation(projectId)) return;
    setNotice(null);
    setError(null);
    try {
      const result = await api.scanProject({ projectId });
      setScanResults((current) => ({ ...current, [projectId]: result }));
      setNotice(`Scan completed: ${result.completed} repositories, ${result.failed} failed.`);
    } catch (reason: unknown) {
      setError(normalizeError(reason, "Project scan could not be started.").message);
    } finally {
      finishProjectOperation(projectId);
    }
  };

  return (
    <section className="view projects-view">
      <header className="view-header">
        <div>
          <p className="eyebrow">Discovery</p>
          <h2>Projects</h2>
        </div>
        <div className="host-picker-entry">
          <button type="button" disabled={rootSelectionBusy} onClick={() => void chooseRoot()}>
            {rootSelectionBusy ? "Choosing root folder…" : "Choose root folder"}
          </button>
          <span>The host selects and validates project roots.</span>
        </div>
      </header>
      {error ? <p className="error-notice">{error}</p> : null}
      {notice ? <p className="success-notice">{notice}</p> : null}
      {projects.length === 0 ? <p>No projects are registered.</p> : null}
      <div className="card-grid">
        {projects.map((project) => {
          const currentDraft = drafts[project.id] ?? draft(project);
          const isBusy = busyProjectIds.has(project.id);
          const result = scanResults[project.id];
          return (
            <article className="card" key={project.id}>
              <header>
                <div>
                  <h3>{project.name}</h3>
                  <p className="path">{project.rootPath}</p>
                </div>
                <button type="button" disabled={isBusy} onClick={() => void scan(project.id)}>
                  Rescan
                </button>
              </header>
              <div className="form-grid">
                <label>
                  Scan depth
                  <input
                    aria-label={`Scan depth for ${project.name}`}
                    type="number"
                    min="0"
                    max="64"
                    value={currentDraft.scanDepth}
                    onChange={(event) =>
                      setDrafts((current) => ({
                        ...current,
                        [project.id]: { ...currentDraft, scanDepth: event.target.value }
                      }))
                    }
                  />
                </label>
                <label>
                  Exclusions, one per line
                  <textarea
                    aria-label={`Exclusions for ${project.name}`}
                    value={currentDraft.exclusions}
                    onChange={(event) =>
                      setDrafts((current) => ({
                        ...current,
                        [project.id]: { ...currentDraft, exclusions: event.target.value }
                      }))
                    }
                  />
                </label>
              </div>
              <button
                type="button"
                disabled={isBusy}
                aria-label={
                  isBusy
                    ? `Saving scan rules for ${project.name}`
                    : `Save scan rules for ${project.name}`
                }
                onClick={() => void saveScanRules(project.id)}
              >
                {isBusy ? "Saving…" : "Save scan rules"}
              </button>
              {result === undefined ? null : (
                <ul className="repository-list">
                  {result.repositories.map((entry) => (
                    <li key={entry.repository.id}>
                      <span>{entry.repository.displayName}</span>
                      <button
                        type="button"
                        onClick={() =>
                          onOpenRepository(entry.repository, { projectId: project.id })
                        }
                      >
                        Open
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </article>
          );
        })}
      </div>
    </section>
  );
}

function draft(project: { scanDepth: number; excludePatterns: string[] }): ProjectDraft {
  return {
    scanDepth: String(project.scanDepth),
    exclusions: project.excludePatterns.join("\n")
  };
}
