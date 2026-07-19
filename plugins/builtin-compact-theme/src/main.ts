import {
  createBrowserTransport,
  createPluginClient,
  type PluginClient
} from "@git-ramus/plugin-sdk";

export function mountCompactPreview(currentDocument: Document, client: PluginClient): void {
  const root = currentDocument.getElementById("app");
  if (root === null) {
    throw new Error("compact theme preview root is missing");
  }
  const section = currentDocument.createElement("section");
  const heading = currentDocument.createElement("h1");
  const status = currentDocument.createElement("p");
  heading.textContent = "Compact Theme";
  status.textContent = "Waiting for the trusted host theme";
  section.append(heading, status);
  root.replaceChildren(section);

  const renderTheme = () => {
    status.textContent = client.currentTheme
      ? `Previewing ${client.currentTheme.name ?? client.currentTheme.themeId}`
      : "Waiting for the trusted host theme";
  };
  client.onThemeChanged(renderTheme);
  void client.ready.then(renderTheme);
}

if (typeof window !== "undefined" && document.getElementById("app") !== null) {
  mountCompactPreview(document, createPluginClient(createBrowserTransport(window)));
}
