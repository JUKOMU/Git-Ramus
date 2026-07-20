import { cloneIntentRequestSchema } from "@git-ramus/contracts";
import type { CloneResult, GitContextRequest, Repository } from "@git-ramus/contracts";
import { useState } from "react";
import type { GitClientApi } from "./api";
import { IdentitiesView } from "./views/IdentitiesView";
import { CloneView } from "./views/CloneView";
import { OverviewView } from "./views/OverviewView";
import { ProjectsView } from "./views/ProjectsView";
import { RepositoryView, type RepositorySelectionSummary } from "./views/RepositoryView";
import { TransportProfilesView } from "./views/TransportProfilesView";
import { WorkspacesView } from "./views/WorkspacesView";

interface AppProps {
  api: GitClientApi;
  route: string;
}

interface RepositorySelection {
  route: string;
  repository: RepositorySelectionSummary;
  context: GitContextRequest;
}

export function App({ api, route }: AppProps) {
  const [repositorySelection, setRepositorySelection] = useState<RepositorySelection | null>(null);
  const selected = repositorySelection?.route === route ? repositorySelection : null;

  if (selected !== null) {
    return (
      <RepositoryView
        api={api}
        context={selected.context}
        repository={selected.repository}
        onBack={() => setRepositorySelection(null)}
      />
    );
  }

  const openRepository = (repository: Repository, context: GitContextRequest) => {
    setRepositorySelection({ route, repository, context });
  };

  const openClonedRepository = (result: CloneResult) => {
    setRepositorySelection({
      route,
      repository: result.repository,
      context: { projectId: result.project.id }
    });
  };

  if (route === "/clone") {
    return <CloneView api={api} intentId={null} onCloned={openClonedRepository} />;
  }
  if (route.startsWith("/clone/")) {
    const parsed = cloneIntentRequestSchema.safeParse({ intentId: route.slice("/clone/".length) });
    if (parsed.success) {
      return (
        <CloneView api={api} intentId={parsed.data.intentId} onCloned={openClonedRepository} />
      );
    }
  }

  switch (route) {
    case "/":
    case "/overview":
      return <OverviewView api={api} onOpenRepository={openRepository} />;
    case "/projects":
      return <ProjectsView api={api} onOpenRepository={openRepository} />;
    case "/workspaces":
      return <WorkspacesView api={api} />;
    case "/identities":
      return <IdentitiesView api={api} />;
    case "/transport-identities":
      return <TransportProfilesView api={api} />;
    default:
      return (
        <section className="view empty-view">
          <h2>Route unavailable</h2>
          <p>The host requested an unsupported Git Client route.</p>
        </section>
      );
  }
}
