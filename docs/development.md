# Git-Ramus development

## Prerequisites

- Node.js 24 or 26 with npm 11.
- Rust 1.88 with `rustfmt` and `clippy`.
- Tauri 2 platform prerequisites for the current operating system.
- Windows builds require the MSVC C++ toolchain; Linux builds require the WebKitGTK development packages used by the CI workflow.

## Setup

```powershell
npm ci
```

## Fast verification

```powershell
npm run check
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npm audit --omit=dev
```

`@wdio/tauri-service` currently pins a native utility package with a missing export, so the root npm overrides keep the compatible `@wdio/native-utils` release. The WebDriver test dependency chain also receives the patched `serialize-javascript` release; both overrides are recorded in `package.json` and the lockfile.

## Desktop development

```powershell
npm run desktop:dev
```

## Native E2E

The E2E build enables the `e2e` Cargo feature, which registers the embedded WebDriver server only in the debug test binary. Release builds do not register or expose that server.

```powershell
npm run build:e2e --workspace @git-ramus/desktop
npm run test:e2e --workspace @git-ramus/desktop
```

The production plugin frame remains a `sandbox="allow-scripts"` cross-origin iframe. The embedded WebDriver implementation cannot inspect that frame's DOM because its native script wrapper cannot cross the browser origin boundary. The journey therefore asserts the real plugin route and navigation, then uses a standard WebDriver script to call the same host command and permission path that the plugin RPC router calls. The test-only service wrapper disables only the stock service's optional auto-focus hook, which requires the richer `tauri-plugin-wdio` frontend bridge; that bridge is not shipped by Git-Ramus.

Built-in plugin resources are generated under `apps/desktop/src-tauri/resources/plugins/` and are intentionally ignored by Git.
