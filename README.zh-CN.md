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

| 路径                   | 用途                                   |
| ---------------------- | -------------------------------------- |
| `apps/desktop/`        | 可信桌面 Shell、Tauri 宿主与原生 E2E   |
| `packages/contracts/`  | 共享 Schema 与 DTO                     |
| `packages/plugin-sdk/` | 类型化插件客户端与浏览器传输           |
| `plugins/`             | 内置业务、Provider、Welcome 与主题插件 |
| `scripts/`             | 构建期插件资源同步                     |
| `docs/`                | 开发指南、已确认设计与实施计划         |

## 路线图

当前已完成的纵向切片包括 Foundation/Microkernel、本地 Git Client、Provider 账户与仓库发现，以及单仓库 Git 网络传输。后续依次推进 Daily Git 高级操作、多仓库同步、ReleaseProvider、插件分发强化、Skills Manager 和发布强化。

实施顺序与安全约束记录在[已确认设计文档](docs/superpowers/specs/)中。

## 参与贡献

Git-Ramus 仍处于活跃的早期开发阶段。请先阅读 [docs/development.md](docs/development.md)，在两个 README 中同步维护功能表述，并在提交 Pull Request 前运行验证命令。

## 许可证

Git-Ramus 使用 [MIT License](LICENSE)。
