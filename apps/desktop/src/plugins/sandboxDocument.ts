const pluginCsp = [
  "default-src 'none'",
  "script-src 'unsafe-inline'",
  "style-src 'unsafe-inline'",
  "img-src data:",
  "font-src data:",
  "connect-src 'none'",
  "object-src 'none'",
  "base-uri 'none'",
  "form-action 'none'"
].join("; ");

export function buildSandboxDocument(html: string): string {
  const meta = `<meta http-equiv="Content-Security-Policy" content="${pluginCsp}">`;
  return `<!doctype html><html><head>${meta}</head><body>${html}</body></html>`;
}

export function buildSandboxUrl(html: string): string {
  return `data:text/html;charset=utf-8,${encodeURIComponent(buildSandboxDocument(html))}`;
}
