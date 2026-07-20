import type { GitContextRequest, Repository } from "@git-ramus/contracts";
import { useState } from "react";
import type { GitClientApi } from "./api";
import { IdentitiesView } from "./views/IdentitiesView";
import { OverviewView } from "./views/OverviewView";
import { ProjectsView } from "./views/ProjectsView";
import { RepositoryView } from "./views/RepositoryView";
import { TransportProfilesView } from "./views/TransportProfilesView";
import { WorkspacesView } from "./views/WorkspacesView";

interface AppProps {
  api: GitClientApi;
  route: string;
}

interface RepositorySelection {
  route: string;
  repository: Repository;
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
