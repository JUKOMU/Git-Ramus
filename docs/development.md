# Git-Ramus development

## Prerequisites

- Node.js 24 or 26 with npm 11.
- Rust 1.88 with `rustfmt` and `clippy`.
- A system Git executable on `PATH`. The Git Client and its native tests invoke Git with argument
  arrays; they never compose shell command strings.
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
npm audit --audit-level=high
```

`@wdio/tauri-service` currently pins a native utility package with a missing export, so the root npm overrides keep the compatible `@wdio/native-utils` release. The WebDriver test dependency chain also receives the patched `serialize-javascript` release; both overrides are recorded in `package.json` and the lockfile.

## Desktop development

```powershell
npm run desktop:dev
```

## Native E2E

The E2E build enables the `e2e` Cargo feature. The embedded WebDriver server,
`e2e_seed_fixture`, and `e2e_seed_provider_fixture` commands are compiled and registered only
when both `e2e` and Rust `debug_assertions` are active. A release build does not contain either
fixture handler, even if it is built with `--features e2e`.

```powershell
npm run build:e2e --workspace @git-ramus/desktop
npm run test:e2e --workspace @git-ramus/desktop
```

Provider discovery unit and integration checks can be run without a network account:

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml providers::
npm run test --workspace @git-ramus/provider-center
```

The native Provider journey uses a compiled mock adapter and `MemorySecretStore` in debug E2E
builds. The fixed PAT exists only transiently in trusted Provider service/IPC memory and is never
returned to the plugin iframe or written to the operating-system keychain. Release builds keep the
real adapters and keychain store and contain no fixture command.

For a release-candidate smoke check, configure and validate one GitHub.com instance, one
GitLab.com instance, and one HTTPS self-managed GitLab instance with an additional CA selected
through the trusted file picker. Reconnect and rotate an account, browse a private repository,
and confirm a local remote binding. Git transport remains the user's normal SSH/GCM path; this
Provider discovery slice does not replace Git authentication or push behavior.

The production plugin frame remains an opaque, cross-origin `sandbox="allow-scripts"` iframe. The
journeys never read its DOM and never add `allow-same-origin`. Host-side data attributes expose only
the contribution route, validated theme ID/density, RPC method names, and completion status; they do
not expose RPC parameters, paths, request/session IDs, credentials, or command results. Git Client
operations use the normal production Tauri commands and DTOs from the host page, matching the
Foundation journey's approach for an opaque frame. The test-only service wrapper disables only the
stock service's optional auto-focus hook, which requires the richer `tauri-plugin-wdio` frontend
bridge; that bridge is not shipped by Git-Ramus.

### Git Client fixture and cleanup

`e2e_seed_fixture` creates a unique direct child of the system temporary directory whose basename
starts with `git-ramus-e2e-`. It creates two Project roots and real Git repositories for included,
excluded, over-depth, and second-directory cases. The included repository contains staged,
unstaged, and untracked changes. Fixture commits supply `user.name`, `user.email`, and
`commit.gpgSign=false` with per-command `-c` arguments, so they do not depend on or modify the
developer's Global Git configuration.

The TypeScript helper strictly validates the native response before use. Its `after` hook removes
Workspace, Identity, and Project records through production commands, then deletes the filesystem
fixture only after proving that the target is a non-symlink direct child of the system temp directory
with the fixed prefix. Never replace this guard with an arbitrary recursive delete.

### Trust, identities, signing, and themes

All repository writes require a recorded Trust decision. Trust gates Stage, Unstage, identity
configuration, and Commit; the E2E journey checks the false-to-true transition before staging one
path. Identity Profiles store Git author and signing policy. Selecting a Profile for Commit applies
that identity without copying credentials into the plugin. If signing is requested and the signing
tool or key is unavailable, the operation returns a user-action error and does not retry as an
unsigned commit.

Theme plugins are data-only: `theme.json` must pass the shared theme schema and cannot inject CSS or
JavaScript. Activating Compact updates the Shell marker and density, then sends the validated token
set to the existing business-plugin iframe through `host:theme-changed`; the iframe is not reloaded.

Built-in plugin resources are generated under `apps/desktop/src-tauri/resources/plugins/` and are intentionally ignored by Git.
