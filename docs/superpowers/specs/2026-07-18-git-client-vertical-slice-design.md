# Git Client Vertical Slice 设计规格

状态：已由用户确认，待实施计划审阅。

## 1. 目标与范围

本阶段实现 Git-Ramus 的第二阶段：Git Client Vertical Slice。它在已完成的 Foundation / Microkernel 之上，提供一个可实际使用的本地 Git 闭环：打开项目、发现仓库、建立跨目录工作区、查看状态和 Diff、选择文件暂存、选择提交身份并完成首条 Commit。

本阶段同时建立全局皮肤插件契约，证明插件可以改变整个应用的视觉风格，而不是只允许业务插件自定义自己的页面。

本阶段包含：

- Rust 原生 Git Engine，调用系统 Git。
- 项目、工作区、仓库和状态快照持久化。
- 递归三层项目扫描与可编辑排除规则。
- 概览、项目、工作区和仓库 Changes 页面。
- 统一 Identity Profile、唯一 Global 身份、仓库级绑定和签名提交。
- 读操作免信任，首次写操作触发仓库 Trust 确认。
- 全局 Theme/Token Contract 与一个可切换的内置 Compact 皮肤插件。
- Rust、TypeScript、真实临时仓库和 Tauri E2E 测试。

明确不包含：

- Fetch、Pull、Push、分支、Merge、Stash、Tag。
- GitHub/GitLab Provider、私有 GitLab 和远程仓库操作。
- Skills Manager、Codex/Claude 安装更新和发布。
- 外部插件下载、安装、升级和卸载。
- 完整 Shell 替换插件；皮肤插件只能改变视觉 Token 和受控布局变体。

## 2. 已确认的设计决策

| 主题 | 决策 |
| --- | --- |
| Git 实现 | Rust 调用系统 `git`，参数数组，不经过 Shell。 |
| 项目扫描 | 递归三层；默认排除 `.git`、`node_modules`、`target`、`dist` 等目录；规则按项目保存并可编辑。 |
| 工作区 | 多个项目的虚拟命名集合，可跨磁盘和目录，不移动真实文件。 |
| 暂存 | 默认不自动暂存；支持逐文件选择和“全部暂存”；Commit 只提交暂存区。 |
| Trust | 未信任仓库可读；首次 Stage/Commit 时提示并持久化信任。 |
| 身份 | 所有 Identity Profile 结构相同；一个唯一 Global 指针；仓库可绑定其他档案；未绑定自然继承 Global。 |
| 签名 | 首版支持 OpenPGP、SSH、X.509 配置；签名工具不可用时明确失败。 |
| UI 插件 | 全局皮肤插件改变 Shell 的视觉风格和受控布局变体；不能注入任意 CSS/JS 或接管完整 Shell。 |
| 业务插件 | Git Client 作为内置插件，通过现有 sandbox/RPC 访问宿主能力。 |

## 3. 总体架构

Git Client UI 继续运行在 `sandbox="allow-scripts"` iframe 中。插件永远不能直接访问文件系统、启动进程或读凭据；所有 Git 和数据库操作由 Rust 宿主完成。

```text
Git Client UI (sandboxed iframe)
        │ typed plugin RPC
        ▼
Tauri Host Commands / Permission Gateway
        │ validates IDs, trust and capabilities
        ▼
GitService
   ├── Project/Workspace/Repository repositories
   ├── IdentityService and ThemeManager
   ├── per-repository write locks
   └── GitEngine
          └── system git (argument array, bounded process)
```

Rust 模块职责：

- `git/engine.rs`：进程启动、参数、环境、超时、取消、输出上限和脱敏。
- `git/parser.rs`：Porcelain v2、NUL 分隔状态、Diff 和 Config 解析。
- `git/repository.rs`：仓库识别、规范化路径、Trust 和写锁。
- `git/service.rs`：项目扫描、工作区关系、快照刷新、暂存和 Commit 编排。
- `identity.rs`：Global/Local 配置读取、档案应用、签名校验和回滚。
- `themes.rs`：主题发现、Schema 校验、当前主题、回退和变更事件。

同一仓库的写操作串行；不同仓库的只读扫描使用有上限的并发。每次写操作完成、取消或失败后都重新读取 Git 真实状态并更新快照。

## 4. 项目扫描与仓库识别

### 4.1 Project

Project 保存：

- `id`、显示名称和规范化根目录。
- `scan_depth`，首版默认 `3`。
- `exclude_patterns`，按项目保存的 glob/目录名规则。
- 创建和更新时间。

打开项目时使用原生 Tauri 文件夹选择器。宿主负责 canonicalize 路径并拒绝不存在或不是目录的路径。

默认排除目录包括：`.git`、`node_modules`、`target`、`dist`、`build`、`.venv`、`vendor`。用户可以在项目设置中增删规则；排除规则只影响扫描，不删除仓库关系。

### 4.2 扫描算法

1. 从 Project 根目录开始，根目录深度为 0。
2. 深度超过 `scan_depth` 的目录不再进入。
3. 先检查当前目录是否为 Git 仓库：普通仓库、`.git` 文件指向的 Worktree、Bare 仓库都交给 `git rev-parse` 确认。
4. 识别为仓库后不继续扫描其内部目录，避免把子目录误当成独立项目。
5. 跳过默认或用户排除目录；不跟随目录 Symlink，防止循环和越界。
6. 对每个仓库使用 canonical real path 去重；同一仓库可被多个 Project 引用。
7. 逐仓库受控并发读取快照；先返回已完成仓库，慢仓库独立显示加载或错误。

扫描识别失败只影响当前候选目录，不终止整个 Project。路径错误、权限错误和 Git 不可用错误通过 `ErrorEnvelope` 返回。

## 5. 数据模型与 SQLite v2 迁移

现有 Foundation 数据库为 `user_version=1`。本阶段新增事务性 `v2` 迁移，迁移重复执行必须幂等。

### 5.1 表与关系

| 表 | 关键字段和约束 |
| --- | --- |
| `projects` | `id`、`name`、唯一 `root_path`、`scan_depth`、`exclude_patterns_json`、时间戳 |
| `workspaces` | `id`、唯一 `name`、时间戳 |
| `workspace_projects` | `workspace_id`、`project_id`、`position`；复合主键和外键级联 |
| `repositories` | `id`、唯一 `canonical_path`、`display_name`、`kind`（normal/bare/worktree）、时间戳 |
| `project_repositories` | `project_id`、`repository_id`、`relative_path`；复合主键 |
| `repository_snapshots` | `repository_id`、HEAD、branch、upstream、ahead/behind、脏状态、计数、刷新时间、错误摘要 |
| `repository_remotes` | `repository_id`、remote name、fetch/push URL；复合唯一键 |
| `trusted_repositories` | `repository_id`、trusted_at、trust_version |
| `identity_profiles` | 名称、`user.name`、`user.email`、`gpg.format`、`user.signingKey`、commit/tag 签署策略、时间戳 |
| `repository_identity_bindings` | `repository_id`、`identity_profile_id`、绑定时间；每仓库最多一个 Git-Ramus 管理绑定 |
| `global_settings` | 单例行 `id=1`，`global_identity_profile_id`、`active_theme_id`、时间戳 |
| `themes` | `theme_id`、来源插件、版本、definition JSON、校验状态、时间戳 |

不变量：

- Project 根路径和 Repository canonical path 唯一。
- Project/Repository、Workspace/Project 均为多对多关系。
- 没有 `repository_identity_bindings` 时，生效身份为 Global 或外部 Local 覆盖。
- Global 角色只由 `global_settings.id=1` 的指针决定；切换时旧档案自动成为普通档案。
- 删除当前 Global 档案必须先转移 Global 指针。
- 快照只保存概览；逐文件 Change 和 Diff 按需读取，不把完整 Diff 持久化。
- 扫描失败保留上一次成功快照，并写入可脱敏错误摘要。

### 5.2 身份档案行为

首次启动读取系统 Git Global 配置；如果数据库没有档案，则创建同结构 Profile 并设置唯一 Global 指针。之后外部修改 Global 配置时显示 Drift，用户可以更新档案或重新应用档案。

应用 Profile 前保存将修改的 Git 配置键快照。Global 使用 `git config --global`，仓库绑定使用 `git config --local`。写入后逐键回读；任何失败都恢复快照，不更新数据库绑定。

签名字段支持 OpenPGP、SSH、X.509。Commit 面板展示有效身份、签名格式和工具可用性；签名请求失败时返回 `UserActionRequired` 错误，不自动改为未签名提交。

## 6. Git Engine

### 6.1 命令矩阵

| 能力 | 命令形态 |
| --- | --- |
| 仓库识别 | `git -C <repo> rev-parse --show-toplevel --is-bare-repository --git-dir` |
| 状态 | `git -C <repo> status --porcelain=v2 -z --branch --untracked-files=all` |
| 未暂存 Diff | `git -C <repo> diff --no-ext-diff --no-textconv --binary -- <paths>` |
| 已暂存 Diff | `git -C <repo> diff --cached --no-ext-diff --no-textconv --binary -- <paths>` |
| 暂存 | `git -C <repo> add -- <paths>` |
| 取消暂存 | `git -C <repo> restore --staged -- <paths>` |
| Commit | `git -C <repo> commit -F -`，message 经 stdin 传入 |
| 配置读取 | `git -C <repo> config --null --get-regexp ...`，分别指定 Global/Local |
| 配置应用 | `git -C <repo> config --global/--local <key> <value>` |

所有用户路径都位于参数数组中，路径参数之后使用 `--`。不拼接 Shell 字符串，不把提交信息放进命令行，不信任用户提供的环境变量。

### 6.2 进程与并发

- 只读命令使用 `--no-optional-locks`（命令不支持时按 Git 版本降级）并受全局并发上限约束。
- 每个进程有超时、stdout/stderr 大小上限和取消句柄。
- 同一 Repository 使用独立写锁；取消后等待子进程退出，再重新读取状态。
- Git 凭据助手仍由系统 Git/GCM/SSH 处理，Git-Ramus 不读取或记录 Token、密码和私钥。
- stderr 只保留脱敏后的用户消息和稳定错误码；原始输出不进入普通日志。

### 6.3 状态解析

Parser 以 NUL 为边界处理 Porcelain v2，覆盖：当前分支、上游、ahead/behind、普通变更、重命名、未跟踪、冲突和路径中的空格/Unicode。解析结果分为：

- `RepositorySnapshot`：概览计数和分支信息。
- `ChangeEntry`：路径、旧路径、状态、是否 staged、是否冲突。
- `DiffDocument`：按需加载的文本/二进制摘要和受限 Diff 内容。

## 7. 插件契约与 Host API

### 7.1 Git Client 插件

新增内置插件 `git-ramus.git-client`。Manifest 声明 `overview`、`projects`、`workspaces` 三个导航贡献；`PluginHost` 保存 `(pluginId, route)` 并把 route 放入 `host:init`。旧插件未提供 route 时默认 `/`，保持兼容。

Git Client 不直接调用 Tauri `invoke`，而是通过插件 RPC 访问以下 Host API：

```text
projects.list/create/updateScanRules/scan
workspaces.list/create/updateMembership/delete
overview.get
repositories.getSnapshot/getChanges/getDiff
repositories.stage/unstage/commit
identities.list/create/update/delete/setGlobal
repositories.bindIdentity/unbindIdentity/getEffectiveIdentity
repositories.trust
```

所有输入使用已登记的 Project、Workspace、Repository、Identity ID；插件不能传任意路径执行 Git。耗时 Scan/Commit 返回 Job ID，通过已有 Job 事件通道同步进度和最终状态。

### 7.2 权限

Git Client Manifest 请求：

- `projects:manage`：Project 创建和扫描规则。
- `workspaces:manage`：Workspace 与 Project 关系。
- `repositories:read`：状态、Changes、Diff、Remotes。
- `repositories:write`：Trust、Stage、Unstage、Commit。
- `identities:read`：Profile 和有效身份。
- `identities:write`：Profile 修改、Global 转移和仓库绑定。

Permission Gateway 在 RPC 路由前检查 capability/resource；宿主再次校验资源 ID、仓库信任和前置状态。

## 8. UI 与主题插件

### 8.1 Git Client 页面

Git Client 在现有深色 Shell 中提供：

- **Overview**：Project/Workspace/Branch/状态筛选，渐进式仓库状态表。
- **Projects**：打开根目录、扫描深度和排除规则、重新扫描、仓库列表。
- **Workspaces**：创建命名 Workspace，加入/移除跨目录 Project。
- **Repository Detail**：路径、分支、Trust、有效身份、Changes、Diff、暂存和 Commit。

Changes 默认分为 Staged、Unstaged、Untracked、Conflicts。用户可以逐文件勾选或全部暂存；Commit 按钮只提交当前 Staged 集合。未实现的 Pull/Push/Merge 等操作不显示为可用按钮。

### 8.2 全局皮肤插件

插件 Manifest 的 `contributions` 增加可选 `theme`：

```json
{
  "theme": {
    "themeId": "git-ramus.theme.compact",
    "definition": "theme.json"
  }
}
```

`theme.json` 只允许 Schema 定义的颜色、字体、间距、圆角、阴影、动效和 `comfortable/compact` 布局变体。宿主 `ThemeManager` 发现并校验主题，保存 `activeThemeId`，用 CSS Variables 应用到整个 Shell，并在主题失效时回退默认主题。

主题插件不能注入任意 CSS/JS，不能修改宿主 DOM、权限或业务状态机。宿主通过 `host:theme-changed` 将令牌同步给所有业务插件；Plugin SDK 暴露 theme 状态和订阅。首版提供默认主题和一个内置 Compact 皮肤插件，主题切换入口位于宿主设置/工具栏。

## 9. 错误、信任与恢复

- `Validation`：路径、扫描规则、提交信息或 Profile 字段错误，就地修正，不创建后台任务。
- `UserActionRequired`：首次 Trust、签名工具不可用或外部身份 Drift，保留上下文等待用户处理。
- `Retryable`：Git 临时锁、超时或读取失败，可重试只读操作。
- `PartialResult`：批量扫描中逐仓库记录成功/失败；本阶段不提供跨仓库写操作。
- `InternalFatal`：数据库迁移或严重存储错误，进入已有只读恢复路径。

Commit/配置写入失败时必须回读 Git 状态。身份应用失败恢复配置键快照；扫描失败不覆盖旧快照；主题校验失败回退默认主题并记录脱敏原因。

## 10. 测试与验收

### 10.1 测试层次

1. TypeScript/Rust 契约测试：Manifest theme、route、Host API 和 ErrorEnvelope。
2. Git Parser 单测：Porcelain v2 fixture、NUL、Unicode、重命名和冲突。
3. SQLite 测试：v2 migration、外键、多对多关系、Global 身份唯一性和快照保留。
4. 真实 Git 临时目录集成测试：Project 扫描、状态、Diff、Stage/Unstage、Trust、Profile 应用、签名 Commit。
5. React 测试：Overview/Projects/Workspaces/Changes、信任弹窗、暂存选择、身份显示和主题同步。
6. Windows/Linux Tauri E2E：打开项目、跨目录 Workspace、切换皮肤、创建身份、暂存并提交首条 Commit。

### 10.2 必须通过的用户旅程

1. 打开根目录，三层扫描发现多个仓库并应用排除规则。
2. 创建跨目录 Workspace 并查看聚合概览。
3. 进入仓库详情，读取 Branch、Changes、Diff 和有效身份。
4. 未信任仓库首次写操作弹出 Trust，确认后可继续。
5. 创建两个身份档案、转移唯一 Global 角色、绑定仓库档案，并由 Git CLI 回读。
6. 逐文件暂存，验证未暂存文件不会进入 Commit。
7. 使用启用签名的档案完成 Commit；工具不可用时显示可操作错误。
8. 激活 Compact 皮肤插件，宿主 Shell、任务中心、Git Client 和 iframe 同步换肤。

### 10.3 性能与安全门槛

- 100 个小仓库扫描受控并发，首批结果渐进显示，不阻塞 Shell。
- Git 命令不经过 Shell 字符串；路径和提交信息均参数化/stdin。
- 未信任仓库不运行 Hooks、外部 Diff 或 Textconv。
- 主题插件无法读取凭据或注入宿主脚本。
- 完成格式、Lint、TypeScript、Rust、真实 Git 集成和 WebView E2E 门禁。

## 11. 后续阶段接口

本阶段留下但不实现：

- Provider Contract 使用 Repository Remote 和 Provider Account 绑定。
- Daily Git 阶段复用 GitEngine 的 Fetch/Pull/Push、Branch、Merge、Stash、Tag 路由。
- Skills Manager 复用 Project/Workspace、Repository 和 Provider Contract。
- 外部插件分发阶段复用 Theme/Slot Contract，但仍不能获得完整 Shell 替换能力。
