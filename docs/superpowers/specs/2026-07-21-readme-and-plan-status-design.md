# README and Historical Plan Status Design

- Date: 2026-07-21
- Status: Approved for implementation planning
- Scope: Public repository entry documentation and historical implementation-plan status only

## 1. Summary

Git-Ramus needs a truthful public entry point and consistent historical plan tracking. The repository currently has a title-only `README.md`, while three completed implementation plans still show every checkbox as open. This change will add paired English and Simplified Chinese README files and reconcile those historical checkboxes with the code and commits already present on `main`.

The documentation will describe Git-Ramus as a **Pre-alpha / Developer Preview**. It will distinguish implemented capabilities from planned MVP work so readers do not mistake Daily Git advanced operations, external plugin distribution, ReleaseProvider, or Skills Manager for available features.

## 2. Goals

1. Make `README.md` the complete English repository entry point.
2. Add a structurally equivalent `README.zh-CN.md` for Simplified Chinese readers.
3. Give users a concise product overview, current feature inventory, source-development quick start, architecture summary, security boundaries, roadmap, contribution guidance, and license link.
4. Reconcile completed historical plan steps with their implementation evidence.
5. Keep commands, paths, versions, and feature claims traceable to files on the current `main` branch.

## 3. Non-goals

- Do not add screenshots, release downloads, installation packages, or claims that binary releases exist.
- Do not change production code, test code, manifests, dependencies, CI behavior, or product scope.
- Do not redesign the detailed development guide or duplicate its real-account smoke procedures in the README files.
- Do not mark planned product phases as complete.
- Do not rewrite historical plan instructions; only add completion metadata and reconcile checkbox state.

## 4. README information architecture

Both README files will use the same section order and equivalent facts:

1. Project title, language switch, CI link/badge, license link, and Pre-alpha notice.
2. A short description of Git-Ramus as a cross-platform, plugin-oriented visual Git management tool.
3. **Implemented today**: Foundation/Microkernel, Project and Workspace organization, repository overview and local Git status/Diff/stage/commit flow, identity profiles, data-only theme plugins, GitHub/GitLab discovery, self-managed GitLab, and single-repository Clone/Fetch/ff-only Pull/safe Push.
4. **Planned for the MVP**: History/Branch/Merge/Stash/Tag/conflict handling, multi-repository synchronization, ReleaseProvider, external plugin distribution hardening, Skills Manager for Codex and Claude Code, and release hardening.
5. Architecture and security boundaries: trusted Tauri/Rust host, sandboxed plugin UI, typed RPC and scoped permissions, system Git/GCM/SSH, OS-protected Provider PATs, and separation of Provider, transport, and commit identities.
6. Source quick start with the exact Node, npm, Rust, Git, and Tauri prerequisites plus `npm ci`, `npm run desktop:dev`, and the primary verification commands.
7. Repository layout covering `apps/`, `packages/`, `plugins/`, `scripts/`, and `docs/`.
8. Roadmap linked to the approved design documents rather than expressed as an unstable percentage.
9. Contribution guidance linked to `docs/development.md` and the MIT license.

The English and Chinese files will link to each other at the top. Technical identifiers, commands, capability names, and file paths remain identical in both languages. Feature lists must be updated in both files in the same change.

## 5. Truth sources

README claims will use the following repository sources:

- Product scope and roadmap: `docs/superpowers/specs/2026-07-17-git-ramus-design.md` and the latest completed slice design.
- Development requirements and commands: `package.json`, `rust-toolchain.toml`, and `docs/development.md`.
- Implemented plugins: the checked-in `plugins/*/plugin.json` manifests.
- CI entry point: `.github/workflows/ci.yml`.
- License: `LICENSE`.

If a claim is present only in a future design and has no matching code or completed plan, it belongs under the planned section.

## 6. Historical plan reconciliation

The following completed plans have stale checkbox state:

| Plan | Current stale state | Completion evidence |
| --- | ---: | --- |
| `2026-07-17-foundation-microkernel.md` | 95 open | Foundation commits `778fc45` through `571ce80`, merged by `125a116` |
| `2026-07-19-git-service-race-and-count-invariants.md` | 11 open | Consistency fixes ending in `dcbaeef`, with later Git Client verification |
| `2026-07-19-provider-account-repository-discovery.md` | 77 open | Provider commits `ee0621a` through `8913b08`, including native E2E and release gates |

Each file will receive a short status block near its header. Every existing unchecked step and final checklist item in those three completed plans will change from `[ ]` to `[x]`. No task text, command, expected result, or code sample will change. The expected post-change counts are `95/0`, `11/0`, and `77/0` completed/open.

Plans that already have zero open checkboxes remain unchanged unless a relative README link requires them. Future roadmap documents and design specifications are not completion trackers and will not be edited.

## 7. Validation

The documentation change is accepted when:

1. `README.md` and `README.zh-CN.md` have matching section structure and reciprocal language links.
2. All repository-relative links in both README files resolve to existing files.
3. Commands and required versions match `package.json`, `rust-toolchain.toml`, and `docs/development.md`.
4. The three reconciled plans have the expected completed/open checkbox counts.
5. `npm run format:check` and `git diff --check` exit successfully.
6. `git status --short` shows only the intended documentation files before commit.

Because this change does not alter executable behavior, code test suites are not required solely for the documentation edit. CI will still run the repository's normal gates after the documentation commit is pushed.

## 8. Delivery

The implementation will use one focused documentation commit after validation. The commit will include the two README files and the three reconciled implementation plans. The design and implementation-plan documents remain separate historical records of the decision and execution process.
