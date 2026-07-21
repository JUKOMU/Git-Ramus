# Git-Ramus

[简体中文](README.zh-CN.md)

[![CI](https://github.com/YozoraTempest/Git-Ramus/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/YozoraTempest/Git-Ramus/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

> [!IMPORTANT]
> Git-Ramus is a **Pre-alpha / Developer Preview**. Build it from source for evaluation. Features listed under the MVP roadmap are not available yet, and no stable binary release is published.

Git-Ramus is a cross-platform, plugin-oriented visual Git management tool built with Tauri 2, React/TypeScript, and Rust. It organizes local repositories through Projects and Workspaces, connects to GitHub and GitLab providers, and keeps privileged Git, filesystem, credential, and secret operations inside a minimal trusted host.

## What works today

- A Tauri/Rust microkernel with SQLite persistence, OS-protected secrets, durable jobs, typed RPC, scoped permissions, and sandboxed plugin UI.
- Repository overview plus Projects (one opened root containing one or more repositories) and Workspaces (named groups of Projects from different directories).
- Trust-gated status, Diff, stage, unstage, and commit flows backed by the system Git executable.
- Reusable commit identity profiles with one global profile, repository bindings, signing policy, and configuration-drift detection.
- Data-only global theme plugins; the bundled Compact Theme changes validated design tokens and density without injecting CSS or JavaScript.
- GitHub.com, GitLab.com, and HTTPS self-managed GitLab repository discovery with PATs kept behind trusted host prompts and the OS secret store.
- Reusable repository-scoped HTTPS/System Git and SSH transport profiles.
- Manual or Provider-initiated Clone, Fetch, fast-forward-only Pull, and safe Push. Force Push and arbitrary RefSpecs are intentionally absent.

## Planned for the MVP

- History, Branch, Merge, Stash, Tag, and conflict-resolution workflows.
- Multi-repository Fetch/Pull/Push, retry of failed items, and noninteractive background checks.
- GitHub/GitLab Release querying, creation, asset upload, and source archives through `ReleaseProvider`.
- External plugin installation, permission review, upgrade, rollback, and distribution hardening.
- Skills Manager for Codex and Claude Code: a managed local Library, Symlink/Copy installation, updates and rollback, plus creator validation, Git, Tag, and Release publishing flows.
- Cross-platform release packaging, performance validation, security review, and real-account acceptance testing.

See the [product design](docs/superpowers/specs/2026-07-17-git-ramus-design.md) and the [next implementation slices](docs/superpowers/specs/2026-07-20-git-transport-network-operations-design.md#22-后续切片).

## Architecture

Git-Ramus follows a minimal trusted-host plus capability-scoped plugin model:

- `apps/desktop`: the trusted Tauri shell, host coordination, native commands, persistence, Git execution, Provider networking, and security boundaries.
- `packages/contracts`: shared Zod schemas and DTO contracts.
- `packages/plugin-sdk`: the browser-side typed plugin client and transport.
- `plugins/git-client`: Projects, Workspaces, repositories, identities, Clone, and network views.
- `plugins/provider-center`, `plugins/provider-github`, and `plugins/provider-gitlab`: Provider account and repository discovery.
- `plugins/builtin-compact-theme`: a data-only global skin plugin.

Built-in and future external plugins share the manifest and permission model. External plugin distribution is still planned; the current runtime ships bundled plugins only.

## Security model

- Plugin pages run in opaque `sandbox="allow-scripts"` iframes and call the host through validated typed RPC.
- Repository writes require an explicit Trust decision and are serialized per repository.
- Provider PATs authenticate hosting APIs only; they never become Git transport credentials.
- HTTPS Git transport delegates to the configured system credential helper/GCM, while SSH delegates to the system Agent and known-hosts policy.
- Commit identities, Provider accounts, and Git transport profiles are separate domains.
- Pull is fast-forward-only. Git-Ramus does not auto-stash, merge, rebase, accept unknown host keys, or expose Force Push.

Read [development.md](docs/development.md) for fixture isolation, authentication boundaries, release probes, and real-account smoke guidance.

## Requirements

- Node.js 24 or 26 and npm 11.
- Rust 1.88 with `rustfmt` and `clippy`.
- Git 2.40 or newer on `PATH`.
- Tauri 2 platform prerequisites for your operating system.

Windows development requires the MSVC C++ toolchain. Linux development requires the WebKitGTK packages installed by the CI workflow.

## Run from source

```powershell
git clone https://github.com/YozoraTempest/Git-Ramus.git
cd Git-Ramus
npm ci
npm run desktop:dev
```

## Verify

```powershell
npm run check
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npm audit --audit-level=high
```

Native desktop E2E instructions and focused Provider/transport commands are documented in [docs/development.md](docs/development.md).

## Repository layout

| Path                   | Purpose                                                          |
| ---------------------- | ---------------------------------------------------------------- |
| `apps/desktop/`        | Trusted desktop shell, Tauri host, and native E2E                |
| `packages/contracts/`  | Shared schemas and DTOs                                          |
| `packages/plugin-sdk/` | Typed plugin client and browser transport                        |
| `plugins/`             | Bundled business, Provider, welcome, and theme plugins           |
| `scripts/`             | Build-time plugin resource synchronization                       |
| `docs/`                | Development guidance, approved designs, and implementation plans |

## Roadmap

The current completed slices are Foundation/Microkernel, the local Git Client vertical slice, Provider account/repository discovery, and single-repository Git transport. Work proceeds through Daily Git advanced operations, multi-repository synchronization, ReleaseProvider, plugin distribution hardening, Skills Manager, and release hardening.

The implementation order and security constraints are tracked in the [approved design documents](docs/superpowers/specs/).

## Contributing

Git-Ramus is in active early development. Start with [docs/development.md](docs/development.md), keep feature claims aligned across both README files, and run the verification commands before opening a pull request.

## License

Git-Ramus is licensed under the [MIT License](LICENSE).
