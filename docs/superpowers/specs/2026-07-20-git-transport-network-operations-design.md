# Git Transport 与单仓库网络操作纵向切片设计

日期：2026-07-20

## 1. 摘要

本纵向切片在已完成的 Foundation、Git Client 本地操作闭环和 Provider 账户/仓库发现能力之上，加入可实际使用的单仓库网络闭环：Clone、Fetch、Fast-forward-only Pull 和安全 Push。

实现采用独立 `GitTransportService`，不把网络认证、Clone 文件生命周期和长任务逻辑继续堆入现有 `GitService`。用户主动触发网络操作时，可信宿主允许系统 Git Credential Manager 显示系统认证界面，并复用已有 SSH Agent；后台和非用户交互路径仍保持非交互。Provider PAT、Git 传输凭据和 Git 提交身份保持三个相互隔离的概念。

本切片同时实现可复用的 SSH/HTTPS 传输身份档案和仓库级绑定。绑定结果写入标准仓库 Git 配置，使命令行和其他 Git 工具可以复用；Git-Ramus 使用应用前快照、应用值和配置指纹避免切换或解绑时覆盖外部修改。

## 2. 与现有设计的关系

本设计细化总体设计 `2026-07-17-git-ramus-design.md` 的 Git 传输身份、Git 执行模型和 Daily Git 第三阶段，并直接承接 `2026-07-19-provider-account-repository-discovery-design.md` 的第一个后续切片。

现有能力保持不变：

- `GitService` 继续负责 Project、Workspace、扫描、Snapshot、Changes、Diff、Stage、Unstage、Commit 和仓库 Trust。
- `ProviderService` 继续负责 GitHub/GitLab 实例、PAT 账户、仓库发现和 Remote Provider 绑定。
- `IdentityService` 继续负责 Commit/Tag 作者及签名身份；它不参与网络认证。
- GitHub/GitLab PAT 只用于 Provider API，不自动写入 GCM，也不作为 Clone/Fetch/Pull/Push 凭据。
- Git Transport 仍调用系统 Git，不引入 libgit2。

## 3. 目标

1. 支持从手动 HTTPS、SSH 或 SCP-like URL Clone 仓库。
2. 支持从 Provider Center 的 GitHub/GitLab 仓库结果发起 Clone，而不向 Provider 插件暴露本地路径或 Git 凭据。
3. Clone 可选择目标目录及现有 Project，成功后自动注册仓库并刷新 Project；也可将新仓库目录创建为新 Project。
4. 支持单仓库、单 Remote 的 Fetch。
5. 支持固定 `--ff-only` 的 Pull，拒绝隐式 Merge、Rebase 或历史改写。
6. 支持向已有 upstream Push；upstream 缺失时由用户选择 Remote/远端分支并设置 upstream。
7. 提供可复用 SSH/HTTPS Transport Profile，并支持仓库级绑定、切换、解绑和 Drift 检测。
8. 用户主动操作可以使用 GCM 系统交互与 SSH Agent；插件、后台路径和自动恢复不能静默触发凭据界面。
9. 所有网络操作提供持久任务状态、阶段进度、取消、超时、错误恢复动作和完成后真实状态刷新。
10. 在失败、取消、应用重启、配置漂移和数据库部分失败时保护现有目录、Git 配置及凭据。

## 4. 非目标

本切片不包含：

- 多仓库批量 Fetch/Pull/Push。
- 自动后台 Fetch 或定时同步。
- Remote 的新增、编辑、重命名和删除 UI。
- History Graph、Branch、Merge、Rebase、Stash、Tag 和冲突解决 UI。
- Force Push、`--force-with-lease`、删除远端分支或任意 RefSpec。
- Submodule 递归 Clone 或 Submodule 管理。
- Git LFS 安装、凭据或文件物化管理。
- GitHub/GitLab Release 查询、创建或资产上传。
- Skills Manager 的安装、更新或发布。
- GitHub Enterprise Server。
- Git-Ramus 自建 HTTPS 密码/PAT 输入框或 SSH 私钥口令存储。
- 自动接受未知 SSH Host Key。
- 应用重启后自动重放网络 Git 命令。

## 5. 已确认决策

| 主题 | 决策 |
| --- | --- |
| 切片范围 | Clone + 单仓库 Fetch/Pull/Push + 仓库级 SSH/HTTPS Transport Profile。 |
| 服务边界 | 新建独立 `GitTransportService`；不把网络流程并入现有 `GitService`。 |
| HTTPS 认证 | 用户主动操作允许系统 GCM UI；秘密由 GCM 管理。 |
| SSH 认证 | 复用 SSH Agent 和结构化 SSH Profile；不保存私钥内容或口令。 |
| 后台认证 | 非交互；不能弹出 GCM 或 SSH 口令界面。 |
| Pull | 固定 `git pull --ff-only`。 |
| Push | 使用已有 upstream；缺失时选择 Remote/分支并 `--set-upstream`；禁止 Force。 |
| Clone 项目集成 | 选择已有 Project 后自动重扫加入，或将 Clone 目录创建为新 Project。 |
| 档案生效方式 | 写标准仓库 `.git/config`，供外部 Git 工具复用。 |
| 凭据隔离 | Provider PAT、Git Transport Credential、Commit Identity 永不隐式合并。 |

## 6. 总体架构

### 6.1 Contract 层

`packages/contracts` 增加以下严格 Schema 与 DTO：

- Transport Profile 的创建、更新、删除影响、列表和脱敏摘要。
- Repository Transport Binding、Effective Transport 和 Drift 状态。
- Clone Intent、Clone Request、Clone Result 和部分完成结果。
- Fetch、Pull、Push Request/Result。
- Upstream Candidate 和 Push Target。
- Network Operation Stage、Progress 和 stable ErrorEnvelope details。

插件请求只接受结构化字段。任何请求都不能包含：

- Git 密码、PAT、OAuth Token 或 SSH 私钥内容。
- 任意环境变量。
- 任意 Git 可执行文件或参数数组。
- 任意 RefSpec。
- Provider Center 发起请求中的本地目录。
- 返回给插件的 SSH 私钥完整路径。

### 6.2 GitEngine 执行策略

现有 `SystemGitRunner` 的 shell-free、参数数组、有界输出、超时和进程树终止能力继续复用，但网络命令必须显式选择宿主管理的执行策略：

1. `LocalNonInteractive`
   - 保持当前本地读取和写入行为。
   - 设置 `GIT_TERMINAL_PROMPT=0` 与 `GCM_INTERACTIVE=Never`。
   - 不允许凭据 UI。

2. `ForegroundNetworkInteractive`
   - 只允许可信宿主为当前用户确认的 Clone/Fetch/Pull/Push 创建。
   - 允许 GCM 使用系统交互模式，并保留 `SSH_AUTH_SOCK` / `SSH_AGENT_PID`。
   - 仍设置 `GIT_TERMINAL_PROMPT=0`，避免终端密码提示阻塞无终端桌面进程。
   - 不继承 `GIT_ASKPASS`、`SSH_ASKPASS`、`GIT_SSH`、`GIT_SSH_COMMAND`、Credential Helper 覆盖或任意 `GIT_CONFIG_*`。
   - Git-Ramus 只能从受控 Profile 构造 SSH 配置；插件不能提供命令字符串。

3. `BackgroundNetworkNonInteractive`
   - 为后续批量/后台切片保留稳定接口。
   - 本切片不创建该类任务。
   - 永远设置 `GCM_INTERACTIVE=Never`。

需要稳定分类 stderr 的网络命令使用宿主管理的 `C` locale；用户可见消息由 Git-Ramus 本地化，不直接展示原始 stderr。

### 6.3 GitTransportService

`GitTransportService` 负责：

- Clone、Fetch、Pull 和 Push 的前置验证及 Git 参数构造。
- Transport Profile 解析和 Remote 传输类型匹配。
- 用户交互策略授权。
- 网络任务、阶段进度、取消和超时。
- Clone Staging 生命周期和注册补偿。
- 操作后 Snapshot、Remote、upstream 和 Provider Binding 刷新。
- 稳定错误映射和日志脱敏。

它依赖：

- `GitNetworkRunner`：运行长时 Git 网络进程并回传受控进度。
- `TransportProfileService`：档案、绑定和 Git Config 应用。
- `GitService` / Repository repositories：仓库、Project、Snapshot、Remote 和 Trust。
- `ProviderService`：只在 Clone 完成后建立已知 Provider Remote 绑定。
- `JobService`：持久任务与操作步骤。
- `CloneIntentBroker`：Provider Center 到 Git Client 的短期脱敏意图。

它不依赖 Provider Secret Store，也不能读取 Provider PAT。

### 6.4 并发边界

- Clone 以规范化最终目标路径作为互斥资源，防止两个任务写入同一目录。
- 已注册仓库的 Fetch/Pull/Push 与 Stage/Unstage/Commit/身份配置共用现有 Repository Write Lock。
- 同一仓库一次只能有一个网络或写操作。
- 不同仓库可以并发，但本切片 UI 不提供批量启动。
- Snapshot/Changes 等读取可以继续受控并发；写操作完成后刷新使用同一操作尾部阶段。

### 6.5 CloneIntentBroker

Provider Center 不能直接执行 Clone，也不能看到本地文件选择结果。流程如下：

1. Provider Center 通过授权 RPC 提交脱敏 Provider Repository 身份、HTTPS/SSH URL 和可选 Provider Account Binding 信息。
2. 可信宿主验证这些字段来自当前 Provider 查询结果，生成短期、一次性、不可猜测的 Clone Intent ID。
3. 宿主导航到 Git Client Clone Route，只传 Intent ID。
4. Git Client 通过 Host API 读取脱敏 Clone 摘要并展示向导。
5. 目标目录选择和 SSH Key 选择由可信宿主原生 Picker 完成；路径不经过 Provider iframe。
6. Intent 在成功消费、取消、过期或插件卸载时销毁。

手动 Clone 不使用 Provider Intent，但经过同一 URL 验证、向导和 Transport Service。

## 7. 持久化模型

迁移版本提升为 v4，并增加以下表。

### 7.1 `transport_profiles`

字段：

- `id TEXT PRIMARY KEY`。
- `display_name TEXT NOT NULL UNIQUE`。
- `kind TEXT NOT NULL CHECK (kind IN ('ssh','https'))`。
- SSH 字段：`ssh_key_path`、`ssh_variant`、`ssh_identities_only`。
- HTTPS 字段：`https_username`、`https_use_http_path`。
- `created_at`、`updated_at`。

表级约束保证：

- SSH Profile 必须有绝对、已规范化的 Key Path；HTTPS 字段为空。
- HTTPS Profile 必须有非空 Username；SSH 字段为空。
- 首版 SSH Variant 只允许系统 OpenSSH 的 `ssh`。
- `ssh_identities_only` 与 `https_use_http_path` 为受 CHECK 约束的布尔整数。

SSH Key Path 不是密钥内容，但仍按敏感本地元数据处理：数据库可持久化完整路径，插件 DTO 只返回文件名和可用状态。

### 7.2 `repository_transport_bindings`

字段：

- `repository_id TEXT PRIMARY KEY`。
- `transport_profile_id TEXT NOT NULL`。
- `before_config_json TEXT NOT NULL`：应用前受管键和值。
- `applied_config_json TEXT NOT NULL`：Git-Ramus 最后写入的受管键和值。
- `applied_config_hash TEXT NOT NULL`：规范化应用值的 SHA-256。
- `drift_status TEXT NOT NULL CHECK (drift_status IN ('clean','drifted'))`。
- `bound_at`、`updated_at`。

外键使用 `ON DELETE RESTRICT`。删除仍被使用的 Profile 必须先完成替换或解绑。

JSON 只保存受管 Git Config 键，不保存凭据。解析时必须使用严格 Host Schema；外部插件不能读取原始 JSON。

### 7.3 Clone 恢复记录

`git_clone_operations` 保存：

- Operation/Job ID。
- 规范化来源摘要与 Provider Intent 关联。
- Staging Path、同级 Ownership Marker Path、Final Path 和目标 Project 选择。
- 当前阶段。
- 文件系统完成、仓库注册、Project 关联、Transport Binding、Provider Binding 各自的完成标志。
- 创建和更新时间。

该表不保存完整凭据、PAT 或 GCM 结果。成功完成后可以保留最小审计摘要并清除 Staging Path；失败记录按恢复策略清理。

## 8. Transport Profile 行为

### 8.1 SSH Profile

SSH Profile 包含：

- Display Name。
- 由可信文件 Picker 选择的私钥路径。
- 固定 SSH Variant `ssh`。
- `IdentitiesOnly`，默认启用。

绑定后由宿主从结构化字段构造并写入：

- `core.sshCommand`。
- `ssh.variant=ssh`。

宿主实现平台专用参数引用，拒绝控制字符、换行和任意附加 SSH 参数。插件不能直接提供 `core.sshCommand`。私钥内容和口令永不进入 SQLite；带口令私钥依赖已配置的 SSH Agent。

### 8.2 HTTPS Profile

HTTPS Profile 包含：

- Display Name。
- 非秘密 Username。
- `credential.useHttpPath`，首版固定启用。

绑定后由宿主写入仓库级：

- `credential.useHttpPath=true`。
- 针对规范化 HTTPS Remote URL 的 `credential.<url>.username`。

密码、PAT 和 OAuth Token 由系统 GCM 管理。Git-Ramus 不创建、读取或导出对应 Secret。

### 8.3 类型匹配

- SSH/SCP Remote 只能使用 SSH Profile。
- HTTPS Remote 只能使用 HTTPS Profile。
- 仓库未绑定 Profile 时使用系统 Git 配置。
- 仓库绑定了与目标 Remote 不同类型的 Profile 时，网络操作返回 `git.transport.profile-mismatch`，不临时回退到其他凭据。
- 一个仓库首版只有一个 Transport Profile。需要不同 Remote 使用不同 SSH 身份时，用户应使用 SSH Host Alias 和相应 Remote URL；Git-Ramus 不自动改写 Alias。

### 8.4 应用、切换和解绑

绑定要求仓库已经 Trust。过程在 Repository Write Lock 内执行：

1. 读取所有受管 Local Git Config 键。
2. 如果没有现有 Git-Ramus Binding 且存在冲突值，展示“仓库自定义”差异并要求用户明确确认替换。
3. 保存应用前值。
4. 逐项写入新值并回读验证。
5. 在 SQLite 事务中写入 Binding、应用值和 Hash。
6. 任一步失败，按应用前快照恢复 Git Config；恢复失败返回 Partial Error 并保留修复记录。

切换 Profile 使用上一 Binding 的原始 `before_config_json` 作为最终解绑基线，而不是把前一个 Profile 的应用值当作用户原始值。

解绑时：

- 如果当前受管键与 `applied_config_hash` 一致，恢复 `before_config_json` 并删除 Binding。
- 如果检测到外部修改，标记 Drift 并停止；用户可选择保留外部配置并删除绑定记录，或明确重新应用 Git-Ramus Profile。
- Git-Ramus 永不静默覆盖 Drift 值。

### 8.5 删除 Profile

删除前返回影响摘要。存在 Binding 时，用户必须逐仓库选择：

- 替换为同类型 Profile。
- 恢复系统配置并解绑。
- 取消删除。

替换 Profile 类型必须与仓库目标 Remote 匹配。所有仓库处理完成后才删除 Profile。

## 9. URL 与 Remote 安全规则

生产入口只接受：

- `https://host/path.git`。
- `ssh://user@host[:port]/path.git`。
- `user@host:path.git` SCP-like URL。

规则：

- HTTPS URL 禁止任何 UserInfo、密码和 Query/Fragment。
- SSH/SCP 允许结构化 Username，但禁止密码。
- Host 使用 IDNA/小写/默认端口规范化；日志只保留脱敏 Host 和 Repository Path。
- 拒绝 `file://`、本地绝对/相对路径、`git://`、自定义 Scheme、`<helper>::` Remote Helper 和含控制字符的值。
- Provider Intent 中的 URL 必须与 Provider Adapter 已验证返回值精确匹配。
- 插件不能改变 Git 可执行文件、`protocol.*.allow` 或 Remote Helper 配置。

测试可以通过注入 Test Runner 或 Debug-only URL Rewrite 使用本地 Bare Repository；该能力必须在 Release Boundary 测试中证明不存在。

## 10. Clone 流程

### 10.1 向导输入

向导展示：

- 来源摘要和 Provider（如果存在）。
- HTTPS/SSH 传输方式选择。
- 有效或可选的同类型 Transport Profile。
- 目标父目录和最终目录名。
- 现有 Project 或“创建新 Project”。
- 来源信任确认。

最终目录必须是用户通过可信 Picker 授权的父目录下的一个不存在子目录。目录名不得为 `.`、`..`、设备名、绝对路径或包含路径分隔注入。

### 10.2 两阶段安全 Clone

1. 创建持久 Job 和 Clone Recovery Record。
2. 为 Final Path 计算同一父目录下、当前不存在的 `.git-ramus-clone-<operation-id>` Staging Path。
3. 在 Staging 旁创建 `.git-ramus-clone-<operation-id>.owner` Ownership Marker；Marker 包含 Operation ID 和规范化 Staging Basename，不包含凭据。Staging Path 本身保持不存在，避免使 `git clone` 因目标非空而失败。
4. 从所选 Profile 构造仅限本次命令的 Host-owned Git Config：SSH 使用受控 `core.sshCommand`，HTTPS 使用受控 Credential Username/Path Scope。该临时配置不写父目录、Global Config 或 Provider Account。
5. 使用受控 URL、`--no-checkout` 和 `--no-recurse-submodules` 获取对象，由 Git 创建 Staging Directory。
6. 验证 Clone 结果是预期普通仓库、`origin` 与请求匹配、`.git` 不为越界链接，且 HEAD/Tree 可读取。
7. 初次 Checkout 使用 Host-owned Empty Hooks Directory，并在 `.git/info/attributes` 写入最高优先级临时规则，取消所有路径的外部 `filter`、`diff`、`merge` 和 `working-tree-encoding` Driver。
8. Checkout 完成后移除临时 Attributes；Git LFS Pointer 不在本切片中自动物化。
9. 原子将 Staging 重命名为 Final Path。
10. 注册 Repository、记录 Trust、通过正常 Binding 流程把所选 Transport Profile 写入新仓库 Local Config、关联 Project、刷新 Snapshot/Remotes。
11. 如果来自 Provider Intent，为 `origin` 建立 Provider Binding。
12. 删除 Ownership Marker，完成 Job 并导航到 Repository Network View。

Staging 与 Final Path 位于同一父目录，保证正常文件系统上的原子重命名。目标在执行期间出现时停止并保留 Staging Recovery Record，不覆盖目标。

### 10.3 Project 处理

- 选择已有 Project：Final Path 必须位于该 Project Root 内且符合扫描深度/排除规则；成功后重扫并验证 Repository 已加入。
- 如果目录会被当前规则排除，向导在执行前要求修改规则或选择其他位置，不在 Clone 后静默留下不可见仓库。
- 选择新 Project：Final Path 作为 Project Root，创建 Project 后执行深度为 0 的首次扫描。
- Project 注册失败不删除已成功 Clone 的目录；任务进入 Partial 状态并提供“重试加入项目”或“保留为未管理目录”。

## 11. Fetch 流程

1. 验证 Repository Context、Trust、Remote 和 Transport Profile。
2. 默认 Remote 为当前 upstream 对应 Remote；没有 upstream 时优先 `origin`；用户可以在现有 Remote 中选择。
3. 执行受控 `git fetch --progress <remote>`。
4. 首版不自动 `--prune`，也不接受插件传入额外参数。
5. 成功、失败或取消后刷新 Remote 与 Snapshot，更新 ahead/behind。

Fetch 不自动重试。认证或网络错误提供用户确认后的重试动作。

## 12. Pull 流程

前置条件：

- HEAD 必须指向本地分支。
- 本地分支必须有 upstream。
- 不存在 unresolved conflict。
- 不存在进行中的 Merge、Rebase、Cherry-pick、Revert 或 Bisect。
- Repository 未被其他写/网络操作占用。

执行：

- 固定调用 `git pull --ff-only --progress`，目标由当前 upstream 决定。
- 不自动 Stash。
- Dirty Worktree 不被一律拒绝；Git 不能安全更新时必须失败且保留原状态。
- 分叉返回 `git.transport.non-fast-forward`，恢复动作是打开状态并等待后续 Merge/Rebase 切片，不自动修改历史。
- 所有结果都重新读取实际 Snapshot。

## 13. Push 流程

### 13.1 已有 upstream

- HEAD 必须是本地分支。
- 宿主解析 upstream Remote 和 Branch。
- 执行等价于将当前 `HEAD` 推送到已解析 upstream 的结构化命令。
- 不允许插件传 RefSpec。

### 13.2 upstream 缺失

- Host API 返回当前 Repository 的 Remote 候选。
- 用户选择 Remote，并输入或确认远端分支名。
- 分支名使用 `git check-ref-format --branch` 和 Host Schema 双重验证。
- 宿主构造 `HEAD:refs/heads/<validated-branch>` 并执行 `--set-upstream`。

### 13.3 禁止行为

- 不暴露 Force、Force-with-lease、Mirror、Delete、All、Tags 或任意 RefSpec 参数。
- Non-fast-forward Push 返回稳定错误和刷新后的 ahead/behind，不提供绕过按钮。

## 14. 任务、进度和取消

每个网络操作创建持久 Job，并记录如下 Operation Step：

- `validating`。
- `awaiting-authentication`。
- `transferring`。
- `checking-out`（仅 Clone）。
- `applying-profile`。
- `registering`（仅 Clone）。
- `refreshing`。
- `completed`、`failed`、`cancelled` 或 `partial`。

Git 进度从受控 stderr 解析为对象计数、字节数或阶段性比例；原始 stderr 不进入插件事件。无法可靠计算总量时 UI 展示不确定进度。

网络任务使用 Host-owned Timeout：

- 等待系统认证最长 5 分钟。
- 传输无进度 Idle Timeout 默认为 120 秒。
- 单次操作 Absolute Timeout 默认为 30 分钟。
- 插件不能扩大这些上限。

取消通过 Windows Job Object 或 Unix Process Group 终止 Git 与认证子进程。取消后必须等待进程树退出，再清理 Clone Staging 或刷新现有仓库。

应用启动时发现仍为 Running 的网络任务：

- 标记 `interrupted` / Action Required。
- 不自动重放 Git 命令或认证。
- Existing Repository 操作只刷新状态。
- Clone 根据 Ownership Marker 和 Recovery Record 提供“安全清理 Staging”“重试 Clone”或“重试注册”。Final Rename 已完成但 Marker 尚未删除时，只能继续注册/绑定或删除 Marker，不能把 Final Path 当作 Staging 清理。

## 15. UI 设计

### 15.1 Transport Identities

Git Client 增加独立导航“传输身份”，与“提交身份”分开。页面支持：

- 创建、编辑和删除 SSH/HTTPS Profile。
- 使用可信 Host Picker 选择 SSH Key。
- 显示 Profile 类型、Username/Key Filename、可用性和绑定仓库数。
- 删除影响和逐仓库替换/解绑。

UI 不显示 SSH Key 完整路径、GCM 凭据或 Provider PAT。

### 15.2 Repository Network View

仓库详情新增 Network View：

- Remote 名称、脱敏 Fetch/Push URL 和传输类型。
- 当前 Branch、upstream、ahead/behind。
- Effective Transport Profile 或“系统 Git 配置”。
- Config Drift 状态和恢复动作。
- Fetch、Pull、Push 操作与任务进度。
- upstream 缺失时的 Push Target 对话框。

### 15.3 Clone View

- Git Client 导航提供手动 Clone。
- Provider Center 仓库行提供 Clone 按钮，通过 Clone Intent 导航到同一 View。
- 目录和密钥选择由 Host Dialog 承担。
- Clone 成功后打开新仓库 Network View；Partial 时展示精确完成步骤和恢复动作。

所有新增 UI 使用语义化 Theme Token，Compact Theme 和未来 UI 风格插件可以统一改变密度、颜色和受控布局；业务插件不能注入宿主 CSS。

## 16. 权限与调用边界

新增能力使用 Repository/Project 资源范围：

- `git.transport.read`：读取脱敏 Profile、Remote、upstream 和任务摘要。
- `git.transport.manage`：创建/修改 Profile 和绑定；需要 Trusted Host 确认文件选择及冲突覆盖。
- `git.network.execute`：Clone/Fetch/Pull/Push。

首版规则：

- Signed built-in Git Client 可以申请上述能力。
- Provider Center 只能创建经过验证的 Clone Intent，不能直接执行 Git、管理 Profile 或选择目录。
- 外部插件即使获得 `git.network.execute` 授权，也不能直接启用 Interactive Policy；每次需要认证的操作必须经过可信 Host 确认。
- 任何 Network RPC 都必须验证调用 Plugin、Manifest 权限、Repository/Project Resource 和 Rate Limit。
- 新增权限不会因插件升级自动扩张。

## 17. 错误模型

至少提供以下稳定错误码：

| Error Code | 含义 | 主要恢复动作 |
| --- | --- | --- |
| `git.transport.authentication-required` | GCM/SSH 缺少可用凭据 | 用户主动重新认证或检查 SSH Agent |
| `git.transport.authentication-cancelled` | 用户取消系统认证 | 返回并按需重试 |
| `git.transport.permission-denied` | Remote 拒绝访问 | 检查账户/Remote 权限 |
| `git.transport.host-key-unverified` | SSH Host Key 未验证 | 外部可信验证后重试 |
| `git.transport.network-unreachable` | DNS/连接/代理失败 | 检查网络后重试 |
| `git.transport.tls` | HTTPS TLS 验证失败 | 检查系统 CA/代理，不允许跳过验证 |
| `git.transport.remote-not-found` | Remote 或仓库不存在 | 检查 Remote |
| `git.transport.upstream-required` | Pull/Push 缺少 upstream | 选择 Push Target 或设置 upstream |
| `git.transport.detached-head` | HEAD 不在本地分支 | 切换/创建分支后重试 |
| `git.transport.operation-in-progress` | Merge/Rebase/Conflict 等状态阻止 Pull | 打开仓库状态处理 |
| `git.transport.non-fast-forward` | Pull/Push 需要历史整合 | 等待 Merge/Rebase 流程 |
| `git.transport.repository-busy` | 同仓库已有写任务 | 等待或取消现有任务 |
| `git.transport.profile-mismatch` | Remote 与 Profile 类型不一致 | 切换 Profile |
| `git.transport.config-drift` | 外部修改了受管 Git Config | 保留外部值或重新应用 |
| `git.transport.destination-exists` | Clone 最终目录已存在 | 选择其他目录 |
| `git.transport.unsafe-path` | Clone/Staging 路径不满足安全边界 | 重新选择目录 |
| `git.transport.cancelled` | 操作已取消 | 刷新后按需重试 |
| `git.transport.timeout` | 认证或传输超时 | 检查网络/认证后重试 |
| `git.transport.partial` | Git、配置、注册或 Provider Binding 部分完成 | 执行 ErrorEnvelope 指定恢复步骤 |

错误详情可以包含脱敏 Remote、Operation ID、Repository ID、失败 Step 和可重试性；不能包含 Secret、完整 SSH Key Path、认证输出或未经脱敏的 URL。

## 18. 失败与恢复规则

### 18.1 Existing Repository 操作

- Fetch/Pull/Push 无论成功、失败、取消或超时，都重新读取 Snapshot 和 Remote。
- 不以 stdout/stderr 推测实际 Branch、upstream 或 ahead/behind。
- 不自动重试 Push/Pull。
- Pull 使用 ff-only，因此失败恢复不需要自动 Merge Abort；如果 Git 报告异常进行中状态，返回 Action Required。

### 18.2 Clone

- 获取对象或初次 Checkout 失败：验证 Ownership Marker、Recovery Record、Canonical Parent、目录名和非 Symlink 后删除专属 Staging，再删除 Marker。
- 取消时先终止并 Reap 进程树，再清理。
- Final Rename 成功后不再自动删除 Final Path。
- Repository 注册失败：保留 Final Path，任务标记 Partial，可重试注册。
- Profile 应用失败：保留仓库，恢复应用前 Git Config，可重试绑定。
- Project 关联失败：保留 Repository，可重试加入或作为未管理目录保留。
- Provider Binding 失败：保留 Repository 和 Project 关联，可重试匹配/绑定。

### 18.3 Config Compensation

- 每次受管 Config 变更在写入前保存 Snapshot。
- 写入后逐键回读；验证失败立即恢复。
- SQLite 提交失败恢复 Git Config。
- 恢复本身失败时写持久 Repair Record，并禁止对该仓库继续切换 Profile，直到用户解决 Drift。

## 19. 安全与隐私

- Git-Ramus 不读取或存储 GCM Secret、SSH 私钥内容或私钥口令。
- Provider PAT 不进入 Transport Service、Git Config、Git Process Environment 或 Network RPC。
- 所有 Git 命令使用参数数组；唯一写入的 `core.sshCommand` 由结构化 Host 字段构造且经过平台引用测试。
- HTTPS 始终验证 TLS；不提供 Skip Verify。
- SSH Host Key 不自动接受或通过 `StrictHostKeyChecking=no` 绕过。
- Clone 不允许 Local/File/Remote Helper Protocol，避免插件借 Git 获得文件系统或任意程序执行。
- 初次 Checkout 禁用 Hooks 和外部 Attribute Driver；不递归 Submodule，不自动运行 LFS Filter。
- Log/Telemetry 对 URL、Username、路径和 stderr 脱敏；默认不记录完整本地目录。
- Profile 完整 Key Path 只在 Rust Host 和可信 Picker 中使用；Plugin DTO 仅返回 Filename/Availability。
- Clone Cleanup 使用与现有 E2E Cleanup 同等级别的路径验证：固定 Prefix、同一父目录、非 Symlink、同级 Ownership Marker 匹配和精确 Operation ID。Marker 永远不放在 `git clone` 的目标目录内。
- Release Build 必须证明 Debug-only URL Rewrite、Fixture Credential 和 Local Bare Mapping 不存在。

## 20. 测试策略

### 20.1 Contract 与权限测试

- 严格解析所有新 Request/Response/Error DTO。
- 拒绝未知字段、任意 RefSpec、凭据、环境变量和完整 Key Path。
- 验证 Provider Center 只能创建 Clone Intent。
- 验证 Repository/Project Resource Scope、Manifest 权限和外部插件交互确认。

### 20.2 SQLite 与 Profile 测试

- v3 到 v4 Migration、外键、CHECK、单仓库唯一绑定。
- SSH/HTTPS 字段互斥。
- Create/Update/Delete Impact。
- 应用、切换、解绑和原始配置恢复。
- Drift 不被覆盖。
- Git Config 写成功但 DB 失败，以及 Config Restore 失败的 Repair Record。

### 20.3 GitEngine 测试

- 三种 Execution Policy 的环境白名单。
- Interactive Policy 允许 GCM/SSH Agent，但清除危险 AskPass/Helper/Git Config 覆盖。
- Host-owned SSH Command 构造和 Windows/Unix 路径引用。
- Progress Parser、stderr Redaction、有界输出、Idle/Absolute Timeout。
- Windows Job Object 与 Unix Process Group 取消认证/传输子进程。

### 20.4 真实 Git 集成测试

使用临时 Bare Remote 和真实系统 Git 覆盖：

- Clone 到 Staging、初次安全 Checkout 和 Final Rename。
- Clone 到已有 Project 与创建新 Project。
- Fetch 更新 Remote Ref 和 ahead/behind。
- Fast-forward Pull 成功。
- 分叉 Pull 返回 Non-fast-forward 且不修改历史。
- Dirty Worktree 可安全快进或由 Git 拒绝而不丢失改动。
- Push 到已有 upstream。
- 缺失 upstream 时选择目标并设置 upstream。
- Non-fast-forward Push 拒绝且没有 Force 路径。
- SSH/HTTPS Profile 写入、外部 `git config` 回读、切换、解绑和 Drift。
- Clone Cancellation、Destination Race、注册 Partial 和安全 Cleanup。

生产 URL Validator 仍拒绝 Local Path。测试通过注入 Test Runner 或 Debug-only Rewrite 使用 Bare Remote。

### 20.5 React 测试

- Transport Identity 列表、创建、编辑、删除影响。
- SSH Key Trusted Picker Broker。
- Repository Network View 的 Remote、upstream、ahead/behind 和按钮状态。
- Clone Wizard 的 Provider/Manual 两条入口、项目选择和信任确认。
- Pull 前置条件、Push Target、Drift 与 Partial Recovery。
- Theme Token 和 Compact Density 同步。
- 断言插件请求不包含 Secret、完整路径或任意 Git 参数。

### 20.6 Native E2E

Debug-only E2E Fixture：

- 创建本地 Bare Remote、两个工作副本和可控 Commit 历史。
- 通过受控 Rewrite 将固定测试 HTTPS URL 映射到 Bare Remote。
- 不访问互联网、不调用真实 GCM、不写用户 Global Git Config。
- 覆盖 Provider Clone Intent → Clone → Project 注册 → Fetch → ff-only Pull → Push/Set Upstream。
- 覆盖 Transport Profile 切换、Task Progress 和安全 Cleanup。
- Windows 与 Linux 运行同一 Journey。

Release Boundary 测试检查 Fixture Command、Rewrite URL、Fixture Token/Path 和 Debug Adapter 均不在 Release Binary 中。

### 20.7 手动冒烟

发布候选至少验证：

- Windows、macOS、Linux 的真实 GitHub HTTPS + GCM。
- SSH Agent 中有口令/无口令 Key 的 GitHub 或 GitLab Clone/Fetch/Pull/Push。
- GitLab.com 和一个自部署 GitLab Remote。
- GCM 取消、SSH Host Key 未验证、权限不足和网络断开。
- 外部 CLI 读取仓库 Profile 配置。

## 21. 验收标准

1. 可以从手动 URL Clone HTTPS/SSH/SCP 仓库。
2. 可以从 Provider Center 创建一次性 Clone Intent，并在 Git Client 完成 Clone；Provider 插件看不到本地路径和 Git 凭据。
3. 可以 Clone 到已有 Project 并自动重扫，也可以把新仓库创建为新 Project。
4. 用户主动操作可以显示 GCM UI；后台/插件不能静默触发认证。
5. 可以创建和复用 SSH/HTTPS Transport Profile，仓库级切换结果可由外部 Git CLI 回读。
6. 未绑定仓库自然使用系统 Git 配置。
7. 外部修改受管 Config 后显示 Drift，Git-Ramus 不静默覆盖。
8. Fetch 更新 Remote 与 ahead/behind。
9. Pull 只允许 Fast-forward，不自动 Merge、Rebase 或 Stash。
10. Push 使用 upstream；缺失时可选择目标并设置 upstream；不存在 Force/任意 RefSpec 路径。
11. 取消或超时会终止完整进程树并刷新真实状态。
12. Clone 失败不会覆盖现有目录；Final Clone 成功后的注册失败保留仓库并提供恢复。
13. Provider PAT、Git Credential 和 Commit Identity 在数据库、日志、IPC 与 Plugin RPC 中保持隔离。
14. Rust、TypeScript、Contract、Migration、真实 Git 集成、React 和 Windows/Linux Native E2E 全部通过。
15. Release Build 不含 Debug-only Transport Fixture 或 URL Rewrite。

## 22. 后续切片

本切片完成后按顺序进入：

1. Daily Git 本地高级操作：History、Branch、Merge、Stash、Tag 和冲突处理。
2. 多仓库批量 Fetch/Pull/Push、失败项重试和后台非交互检查。
3. `ReleaseProvider`：GitHub/GitLab Release 查询、创建、资产上传和源归档。
4. Plugin Distribution Hardening。
5. Skills Manager 的使用者安装/更新与创作者发布。

后续切片必须继续保持 Provider PAT、Git Transport Credential 和 Commit Identity 相互隔离，也不能通过批量或后台能力绕过本设计的用户交互及 Trust 边界。
