import "./style.css";
import { createBrowserTransport, createPluginClient } from "@git-ramus/plugin-sdk";

interface WelcomeClient {
  ready: Promise<unknown>;
  request(method: string, params: unknown): Promise<unknown>;
}

interface AppInfo {
  name: string;
  version: string;
}

export function mountWelcome(currentDocument: Document, client: WelcomeClient) {
  const root = currentDocument.getElementById("app");
  if (root === null) {
    throw new Error("welcome plugin root is missing");
  }
  root.innerHTML = `
    <section class="welcome">
      <span class="eyebrow">Built-in plugin</span>
      <h1>Welcome to Git-Ramus</h1>
      <p id="connection">Connecting to the trusted host</p>
      <button id="echo" type="button">Run background echo task</button>
      <p id="job" aria-live="polite"></p>
    </section>
  `;
  void client.ready.then(async () => {
    const info = await client.request("app.getInfo", {});
    if (!isAppInfo(info)) {
      throw new Error("app.getInfo returned an invalid response");
    }
    const connection = currentDocument.getElementById("connection");
    if (connection !== null) {
      connection.textContent = `Connected to ${info.name} ${info.version}`;
    }
  });
  currentDocument.getElementById("echo")?.addEventListener("click", () => {
    void client.request("tasks.startEcho", { message: "Hello from Welcome" }).then((job) => {
      if (!isJobReference(job)) {
        throw new Error("tasks.startEcho returned an invalid response");
      }
      const output = currentDocument.getElementById("job");
      if (output !== null) {
        output.textContent = `Created job ${job.id}`;
      }
    });
  });
}

function isAppInfo(value: unknown): value is AppInfo {
  return (
    typeof value === "object" &&
    value !== null &&
    "name" in value &&
    typeof value.name === "string" &&
    "version" in value &&
    typeof value.version === "string"
  );
}

function isJobReference(value: unknown): value is { id: string } {
  return (
    typeof value === "object" && value !== null && "id" in value && typeof value.id === "string"
  );
}

if (typeof window !== "undefined" && document.getElementById("app") !== null) {
  mountWelcome(document, createPluginClient(createBrowserTransport(window)));
}
