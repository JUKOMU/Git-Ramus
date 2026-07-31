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
`e2e_seed_fixture`, `e2e_seed_provider_fixture`, and transport fixture commands are compiled and
registered only when both `e2e` and Rust `debug_assertions` are active. A release build does not
contain those fixture handlers or the transport URL rewrite, even if it is built with
`--features e2e`.

```powershell
npm run build:e2e --workspace @git-ramus/desktop
npm run test:e2e:plugin-forms --workspace @git-ramus/desktop
npm run test:e2e --workspace @git-ramus/desktop
```

Run `build:e2e` before either native test command. The sandboxed-form test uses the external
WebDriver provider and requires `tauri-driver`; its configuration can install `tauri-driver`
automatically, and on Windows it can also download EdgeDriver. It switches into the real opaque
`sandbox="allow-scripts"` Git Client iframe, fills and clicks the Identity form, observes only the
Host-side RPC method/status markers, and confirms the result with the production
`git_identity_list` command against the isolated E2E database. It does not add `allow-forms` or
`allow-same-origin`, bypass RPC authorization, or expose form values, credentials, RPC parameters,
database paths, or command results through Host DOM attributes.

Run the external sandboxed-form test and the four embedded journeys serially. When the external run
finishes, its launcher service shuts down the external driver; on Windows it terminates the exact
driver process trees tracked by that service. This releases the external driver port before the
embedded suite starts on port `4445`, so the normal embedded journeys can run immediately afterward.

Provider discovery unit and integration checks can be run without a network account:

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml providers::
npm run test --workspace @git-ramus/provider-center
```

### Git transport focused verification

These commands cover the shared contracts, Transport Profile lifecycle, real-Git orchestration,
Git Client views, and the native transport journey without requiring a real network account:

```powershell
npm run test --workspace @git-ramus/contracts -- src/__tests__/contracts.test.ts
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::transport::profile_service::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test git_transport_integration
npm run test --workspace @git-ramus/git-client -- src/__tests__/TransportProfilesView.test.tsx src/__tests__/CloneView.test.tsx src/__tests__/RepositoryNetworkPanel.test.tsx
npm run build:e2e --workspace @git-ramus/desktop
npm run test:e2e:transport --workspace @git-ramus/desktop
```

The deterministic transport journey uses real local Git repositories and a local bare remote. Its
fixed HTTPS URL is rewritten only inside a sealed Debug-E2E Git configuration. It never contacts or
modifies a real account, credential helper, SSH configuration, known-hosts file, or Global Git
configuration. Release builds contain neither the rewrite nor its fixture data.

### Git authentication and safety boundaries

- Provider PATs authenticate repository discovery APIs only. They are not Git credentials and are
  never handed to Clone, Fetch, Pull, Push, Git Credential Manager (GCM), or SSH.
- HTTPS transport runs system Git. GCM or another configured system credential helper may display
  UI only for a user-confirmed foreground operation; background operations are noninteractive.
- SSH Profiles select a key file through the trusted picker and reuse the configured SSH Agent for
  passphrase-backed keys. Git-Ramus stores neither private-key contents nor passphrases and never
  auto-accepts an unknown host key. Verify the fingerprint outside the app and update the user's
  known-hosts configuration before retrying.
- Commit Identity Profiles configure author/signing identity only. They do not contain Provider or
  transport credentials.
- Pull always uses fast-forward-only behavior. Divergence is reported for user action; Git-Ramus
  does not merge, rebase, or auto-stash. Push has no Force or arbitrary RefSpec path.

### Real-account transport smoke check

Use disposable repositories and branches. First confirm the same Clone/Fetch/Push works with system
Git in a terminal, then create the corresponding HTTPS/System Git or SSH Transport Profile in
Git-Ramus; load passphrase-backed test keys into the Agent. Trust the repository explicitly and
confirm each foreground network operation in the trusted Host UI.

1. **GitHub HTTPS with GCM:** clone a private repository through the Git Client wizard, allow the
   system GCM prompt, Fetch, make a disposable commit, Push with upstream, and Pull a remote
   fast-forward. Confirm `origin` contains no token and no Provider PAT prompt is involved.
2. **GitHub or GitLab SSH Agent:** load the test key into the system Agent and verify the server
   fingerprint before adding it to known hosts. Clone via SSH, then Fetch and Push. With an
   intentionally unknown test host, confirm the operation stops instead of accepting its key.
3. **GitLab.com:** repeat the private-repository flow with both repository discovery and Git
   transport configured. Rotating or removing the Provider PAT must not alter the bound Git
   Transport Profile or its system credential-helper behavior.
4. **Self-managed GitLab:** add one HTTPS or SSH GitLab Remote, including the trusted corporate CA
   or verified SSH host key through the operating-system/system-Git trust setup. Browse a private
   repository, hand it to the Clone wizard, then Fetch/Pull/Push. Confirm the public remote URL is
   retained and no CA path, credential, or local destination appears in plugin data.
5. **Safety cases:** create a divergent branch and confirm Pull refuses it without changing HEAD;
   confirm no Force Push action exists; modify a Git config value managed by a bound Profile and
   confirm drift is reported rather than overwritten silently.

Run the smoke set on each release-candidate platform:

- **Windows:** Git for Windows, GCM, and the Windows/OpenSSH Agent.
- **macOS:** system Git or the supported Git distribution, its configured credential helper, and
  `ssh-agent`/Keychain-backed keys.
- **Linux:** system Git, an explicitly configured GCM/credential helper for HTTPS, and `ssh-agent`.

Record the Git version, credential-helper/Agent type, hosting target, transport type, and the result
of Clone, Fetch, fast-forward Pull, Push, cancellation, and unknown-host handling. Never paste
credentials, full key paths, or fixture cleanup tokens into the report.

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
four embedded journeys (Foundation, Git Client, Provider, and Git Transport) never read its DOM and
never add `allow-same-origin`. Host-side data attributes expose only the contribution route,
validated theme ID/density, RPC method names, and completion status; they do not expose RPC
parameters, paths, request/session IDs, credentials, or command results. Git Client operations use
the normal production Tauri commands and DTOs from the host page, matching the Foundation journey's
approach for an opaque frame. The test-only embedded service wrapper disables only the stock
service's optional auto-focus hook, which requires the richer `tauri-plugin-wdio` frontend bridge;
that bridge is not shipped by Git-Ramus.

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
