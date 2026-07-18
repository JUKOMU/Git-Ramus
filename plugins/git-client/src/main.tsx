import { createBrowserTransport, createPluginClient } from "@git-ramus/plugin-sdk";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { createGitClientApi } from "./api";
import "./style.css";

const rootElement = document.getElementById("root");
if (rootElement === null) {
  throw new Error("Git Client root element is missing");
}

const root = createRoot(rootElement);
const client = createPluginClient(createBrowserTransport());
const api = createGitClientApi(client);

root.render(<p className="connection-state">Connecting to Git-Ramus…</p>);

void client.ready
  .then((init) => {
    root.render(<App api={api} route={init.route ?? "/"} />);
  })
  .catch(() => {
    root.render(
      <section className="view empty-view">
        <h2>Connection unavailable</h2>
        <p>The Git Client could not establish a host session.</p>
      </section>
    );
  });

window.addEventListener("pagehide", () => client.dispose(), { once: true });
