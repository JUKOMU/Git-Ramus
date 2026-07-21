# README and Historical Plan Status Reconciliation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Publish truthful English and Simplified Chinese repository entry documents and reconcile three completed historical implementation plans with their actual status on `main`.

**Architecture:** `README.md` is the English default entry point and `README.zh-CN.md` is its structure-equivalent Chinese counterpart. Historical plans keep their original instructions, gain concise completion evidence, and receive mechanical checkbox reconciliation only after the corresponding commits and tests have been verified.

**Tech Stack:** Markdown, PowerShell, Prettier, Git

---

## File map

- Modify `README.md`: complete English project entry point.
- Create `README.zh-CN.md`: complete Simplified Chinese entry point with matching facts and structure.
- Modify `docs/superpowers/plans/2026-07-17-foundation-microkernel.md`: add completion evidence and reconcile 95 checkboxes.
- Modify `docs/superpowers/plans/2026-07-19-git-service-race-and-count-invariants.md`: add completion evidence and reconcile 11 checkboxes.
- Modify `docs/superpowers/plans/2026-07-19-provider-account-repository-discovery.md`: add completion evidence and reconcile 77 checkboxes.
- Modify `docs/superpowers/plans/2026-07-21-readme-and-plan-status-reconciliation.md`: mark this plan complete only after every verification command passes.

### Task 1: Reconcile completed historical plans

**Files:**

- Modify: `docs/superpowers/plans/2026-07-17-foundation-microkernel.md`
- Modify: `docs/superpowers/plans/2026-07-19-git-service-race-and-count-invariants.md`
- Modify: `docs/superpowers/plans/2026-07-19-provider-account-repository-discovery.md`

- [x] **Step 1: Capture the stale checkbox counts**

Run:

```powershell
$files = @(
  "docs/superpowers/plans/2026-07-17-foundation-microkernel.md",
  "docs/superpowers/plans/2026-07-19-git-service-race-and-count-invariants.md",
  "docs/superpowers/plans/2026-07-19-provider-account-repository-discovery.md"
)
foreach ($file in $files) {
  $content = Get-Content $file
  [PSCustomObject]@{
    File = Split-Path $file -Leaf
    Done = ($content | Select-String '^- \[x\]').Count
    Open = ($content | Select-String '^- \[ \]').Count
  }
}
```

Expected:

```text
2026-07-17-foundation-microkernel.md                  Done 0  Open 95
2026-07-19-git-service-race-and-count-invariants.md   Done 0  Open 11
2026-07-19-provider-account-repository-discovery.md   Done 0  Open 77
```

- [x] **Step 2: Add exact completion evidence below each plan header**

Add this paragraph after the Architecture/Tech Stack block in the Foundation plan:

```markdown
**Status:** Completed on `main`. Implemented by commits `778fc45` through `571ce80`, verified by `82f5b6e` and `571ce80`, and integrated by merge commit `125a116`.
```

Add this paragraph after the Architecture/Tech Stack block in the race-and-count plan:

```markdown
**Status:** Completed on `main`. The project-lock, atomic repository creation, and counter invariants were delivered by the Git Client consistency fixes ending in `dcbaeef` and remain covered by the current Rust integration suite.
```

Add this paragraph after the Architecture/Tech Stack block in the Provider plan:

```markdown
**Status:** Completed on `main`. Implemented by commits `ee0621a` through `3bf44dc`, hardened through `8913b08`, and covered by Provider unit, integration, native E2E, and release-boundary gates.
```

- [x] **Step 3: Mechanically mark only the three completed plans**

Run this PowerShell script from the repository root:

```powershell
$files = @(
  "docs/superpowers/plans/2026-07-17-foundation-microkernel.md",
  "docs/superpowers/plans/2026-07-19-git-service-race-and-count-invariants.md",
  "docs/superpowers/plans/2026-07-19-provider-account-repository-discovery.md"
)
$utf8 = [System.Text.UTF8Encoding]::new($false)
foreach ($file in $files) {
  $path = (Resolve-Path $file).Path
  $content = [System.IO.File]::ReadAllText($path)
  $updated = $content.Replace("- [x]", "- [x]")
  [System.IO.File]::WriteAllText($path, $updated, $utf8)
}
```

Do not run the replacement against the whole `docs/superpowers/plans` directory; active plans must retain open checkboxes.

- [x] **Step 4: Verify the reconciled counts**

Repeat the Step 1 command.

Expected:

```text
2026-07-17-foundation-microkernel.md                  Done 95  Open 0
2026-07-19-git-service-race-and-count-invariants.md   Done 11  Open 0
2026-07-19-provider-account-repository-discovery.md   Done 77  Open 0
```

### Task 2: Write the English repository entry point

**Files:**

- Modify: `README.md`
- Reference: `docs/development.md`
- Reference: `docs/superpowers/specs/2026-07-17-git-ramus-design.md`
- Reference: `docs/superpowers/specs/2026-07-20-git-transport-network-operations-design.md`

- [x] **Step 1: Replace the title-only README with the approved structure**

Write `README.md` with these exact sections and facts:

```markdown
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

| Path | Purpose |
| --- | --- |
| `apps/desktop/` | Trusted desktop shell, Tauri host, and native E2E |
| `packages/contracts/` | Shared schemas and DTOs |
| `packages/plugin-sdk/` | Typed plugin client and browser transport |
| `plugins/` | Bundled business, Provider, welcome, and theme plugins |
| `scripts/` | Build-time plugin resource synchronization |
| `docs/` | Development guidance, approved designs, and implementation plans |

## Roadmap

The current completed slices are Foundation/Microkernel, the local Git Client vertical slice, Provider account/repository discovery, and single-repository Git transport. Work proceeds through Daily Git advanced operations, multi-repository synchronization, ReleaseProvider, plugin distribution hardening, Skills Manager, and release hardening.

The implementation order and security constraints are tracked in the [approved design documents](docs/superpowers/specs/).

## Contributing

Git-Ramus is in active early development. Start with [docs/development.md](docs/development.md), keep feature claims aligned across both README files, and run the verification commands before opening a pull request.

## License

Git-Ramus is licensed under the [MIT License](LICENSE).
```

- [x] **Step 2: Check English claims against repository sources**

Run:

```powershell
Select-String -Path README.md -Pattern 'Pre-alpha|Skills Manager|ff-only|Node.js 24 or 26|Rust 1.88|Git 2.40'
```

Expected: each approved maturity, scope, safety, and version statement is present; Skills Manager appears only under planned work.

### Task 3: Write the Simplified Chinese repository entry point

**Files:**

- Create: `README.zh-CN.md`
- Reference: `README.md`

- [x] **Step 1: Create a structure-equivalent Chinese README**

Create `README.zh-CN.md` with this complete content:

```markdown
# Git-Ramus

[English](README.md)

[![CI](https://github.com/YozoraTempest/Git-Ramus/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/YozoraTempest/Git-Ramus/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

> [!IMPORTANT]
> Git-Ramus 目前处于 **Pre-alpha / Developer Preview** 阶段。请从源码构建后进行评估。MVP 路线图中的功能尚不可用，当前也没有稳定的二进制发行版本。

Git-Ramus 是一款面向 Windows、macOS 和 Linux 设计的插件化可视 Git 管理工具，使用 Tauri 2、React/TypeScript 与 Rust 构建。它通过 Project 和 Workspace 组织本地仓库，通过 Provider 连接 GitHub 与 GitLab，并将高权限的 Git、文件系统、凭据和密钥操作限制在最小可信宿主中。

## 当前可用能力

- 基于 Tauri/Rust 的微内核，提供 SQLite 持久化、操作系统保护的密钥、持久任务、类型化 RPC、范围化权限与沙箱插件 UI。
- 仓库概览，以及 Project（用户打开的一个根目录，其中可包含一个或多个仓库）和 Workspace（由不同目录下多个 Project 组成的命名集合）。
- 基于系统 Git 的状态、Diff、暂存、取消暂存和提交，并由仓库 Trust 决策保护写操作。
- 可复用的提交身份档案，支持唯一全局身份、仓库绑定、签名策略和配置漂移检测。
- 仅包含数据的全局主题插件；内置 Compact Theme 通过已验证的设计令牌和密度改变界面风格，不注入 CSS 或 JavaScript。
- 支持 GitHub.com、GitLab.com 和 HTTPS 私有部署 GitLab 的仓库发现；PAT 仅通过可信宿主提示获取，并保存在操作系统密钥存储中。
- 可复用、仓库级的 HTTPS/System Git 与 SSH 传输档案。
- 手动或由 Provider 发起的 Clone，以及 Fetch、仅 fast-forward 的 Pull 和安全 Push。产品有意不提供 Force Push 和任意 RefSpec。

## MVP 规划

- History、Branch、Merge、Stash、Tag 和冲突处理工作流。
- 多仓库 Fetch/Pull/Push、失败项重试和非交互后台检查。
- 通过 `ReleaseProvider` 查询和创建 GitHub/GitLab Release、上传资产并生成源代码归档。
- 外部插件安装、权限审阅、升级、回滚与分发强化。
- 面向 Codex 和 Claude Code 的 Skills Manager：统一本地 Library、Symlink/Copy 安装、更新与回滚，以及创作者校验、Git、Tag 和 Release 发布流程。
- 跨平台发布打包、性能验证、安全审阅和真实账户验收测试。

参阅[产品设计](docs/superpowers/specs/2026-07-17-git-ramus-design.md)和[后续实施切片](docs/superpowers/specs/2026-07-20-git-transport-network-operations-design.md#22-后续切片)。

## 架构

Git-Ramus 采用“最小可信宿主 + 能力范围化插件”模型：

- `apps/desktop`：可信 Tauri Shell、宿主协调、原生命令、持久化、Git 执行、Provider 网络访问与安全边界。
- `packages/contracts`：共享 Zod Schema 与 DTO 契约。
- `packages/plugin-sdk`：浏览器侧类型化插件客户端与传输层。
- `plugins/git-client`：Project、Workspace、仓库、身份、Clone 与网络操作页面。
- `plugins/provider-center`、`plugins/provider-github` 和 `plugins/provider-gitlab`：Provider 账户与仓库发现。
- `plugins/builtin-compact-theme`：仅包含数据的全局界面风格插件。

内置插件与未来的外部插件共用 Manifest 和权限模型。外部插件分发仍在规划中；当前运行时只交付内置插件。

## 安全模型

- 插件页面运行在不透明的 `sandbox="allow-scripts"` iframe 中，并通过经过验证的类型化 RPC 调用宿主。
- 仓库写操作需要显式 Trust 决策，并按仓库串行执行。
- Provider PAT 只用于托管平台 API 认证，不会成为 Git 传输凭据。
- HTTPS Git 传输委托给系统凭据助手/GCM；SSH 委托给系统 Agent 和 known-hosts 策略。
- 提交身份、Provider 账户与 Git 传输档案属于相互隔离的领域。
- Pull 仅允许 fast-forward。Git-Ramus 不会自动 Stash、Merge、Rebase、接受未知 Host Key，也不提供 Force Push。

关于测试夹具隔离、认证边界、Release 探针和真实账户冒烟验证，请阅读 [development.md](docs/development.md)。

## 环境要求

- Node.js 24 或 26，以及 npm 11。
- Rust 1.88，并安装 `rustfmt` 和 `clippy`。
- `PATH` 中存在 Git 2.40 或更高版本。
- 当前操作系统对应的 Tauri 2 平台依赖。

Windows 开发需要 MSVC C++ 工具链；Linux 开发需要 CI 工作流中安装的 WebKitGTK 软件包。

## 从源码运行

```powershell
git clone https://github.com/YozoraTempest/Git-Ramus.git
cd Git-Ramus
npm ci
npm run desktop:dev
```

## 验证

```powershell
npm run check
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npm audit --audit-level=high
```

原生桌面 E2E 说明以及 Provider/Transport 定向验证命令位于 [docs/development.md](docs/development.md)。

## 仓库结构

| 路径 | 用途 |
| --- | --- |
| `apps/desktop/` | 可信桌面 Shell、Tauri 宿主与原生 E2E |
| `packages/contracts/` | 共享 Schema 与 DTO |
| `packages/plugin-sdk/` | 类型化插件客户端与浏览器传输 |
| `plugins/` | 内置业务、Provider、Welcome 与主题插件 |
| `scripts/` | 构建期插件资源同步 |
| `docs/` | 开发指南、已确认设计与实施计划 |

## 路线图

当前已完成的纵向切片包括 Foundation/Microkernel、本地 Git Client、Provider 账户与仓库发现，以及单仓库 Git 网络传输。后续依次推进 Daily Git 高级操作、多仓库同步、ReleaseProvider、插件分发强化、Skills Manager 和发布强化。

实施顺序与安全约束记录在[已确认设计文档](docs/superpowers/specs/)中。

## 参与贡献

Git-Ramus 仍处于活跃的早期开发阶段。请先阅读 [docs/development.md](docs/development.md)，在两个 README 中同步维护功能表述，并在提交 Pull Request 前运行验证命令。

## 许可证

Git-Ramus 使用 [MIT License](LICENSE)。
```

- [x] **Step 2: Verify heading-level and link parity**

Run:

```powershell
$enLevels = Get-Content README.md | Where-Object { $_ -match '^#{1,6} ' } | ForEach-Object { ($_ -split ' ')[0] }
$zhLevels = Get-Content README.zh-CN.md | Where-Object { $_ -match '^#{1,6} ' } | ForEach-Object { ($_ -split ' ')[0] }
$difference = Compare-Object $enLevels $zhLevels
if ($difference) { throw "README heading structures differ" }
```

Expected: exit code 0 with no difference output.

### Task 4: Validate and deliver the documentation change

**Files:**

- Modify: `docs/superpowers/plans/2026-07-21-readme-and-plan-status-reconciliation.md`
- Verify: `README.md`
- Verify: `README.zh-CN.md`
- Verify: the three reconciled historical plans

- [x] **Step 1: Validate repository-relative README links**

Run:

```powershell
$files = @("README.md", "README.zh-CN.md")
foreach ($file in $files) {
  $base = Split-Path (Resolve-Path $file).Path -Parent
  $html = (ConvertFrom-Markdown -Path $file).Html
  foreach ($match in [regex]::Matches($html, 'href="([^"]+)"')) {
    $target = $match.Groups[1].Value
    if ($target -match '^(https?://|#)') { continue }
    $path = ($target -split '#', 2)[0]
    if (-not (Test-Path (Join-Path $base $path))) {
      throw "$file contains a missing relative link: $target"
    }
  }
}
```

Expected: exit code 0 with no missing-link error.

- [x] **Step 2: Run formatting and whitespace gates**

Run:

```powershell
npm run format:check
git diff --check
```

Expected: Prettier reports all matched files use its style and `git diff --check` exits 0.

- [x] **Step 3: Inspect the documentation-only diff**

Run:

```powershell
git status --short
git diff --stat
git diff -- README.md README.zh-CN.md docs/superpowers/plans/2026-07-17-foundation-microkernel.md docs/superpowers/plans/2026-07-19-git-service-race-and-count-invariants.md docs/superpowers/plans/2026-07-19-provider-account-repository-discovery.md
```

Expected: only the two README files, the three reconciled historical plans, and this active implementation plan are modified. No production, test, manifest, dependency, or CI file is changed.

- [x] **Step 4: Mark this plan complete and re-run final checks**

Change every remaining checkbox in this implementation plan from `[ ]` to `[x]` only after Steps 1-3 pass. Then run:

```powershell
npm run format:check
git diff --check
git status --short
```

Expected: both checks exit 0 and status lists only the intended documentation files.

- [x] **Step 5: Commit the reconciled documentation**

Run:

```powershell
git add README.md README.zh-CN.md docs/superpowers/plans/2026-07-17-foundation-microkernel.md docs/superpowers/plans/2026-07-19-git-service-race-and-count-invariants.md docs/superpowers/plans/2026-07-19-provider-account-repository-discovery.md docs/superpowers/plans/2026-07-21-readme-and-plan-status-reconciliation.md
git commit -m "docs: publish bilingual project readme"
```

Expected: one documentation-only commit succeeds.
