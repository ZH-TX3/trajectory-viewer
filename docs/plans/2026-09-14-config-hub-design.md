# Config Hub — 统一配置管理设计

> 日期：2026-09-14
> 状态：设计已确认，待实现
> 参考实现：cc-switch 3.14.1（`D:/DSH`）

## 1. 背景与目标

当前用户机器上，多个 AI CLI 工具各自维护一套配置，互不相通：

| 工具 | 配置根 | 提示词文件 | skills 目录 | agents 目录 |
|------|--------|-----------|------------|------------|
| AI Code (Claude) | `~/.claude` | `CLAUDE.md` | `~/.claude/skills`（8 个**实体副本**） | `~/.claude/agents/*.md` |
| Codex | `~/.codex` | `AGENTS.md` | `~/.codex/skills`（1 个软链接） | 无 |
| DSH | `~/.dsh` | `AGENTS.md` | 无 | `.agent-presets/<name>/`（目录式，暂不纳入） |
| OpenCode | `~/.config/opencode` | `AGENTS.md` | `~/.config/opencode/skills`（空） | 无 |

而 `~/.agents/`（Agent Skills 社区统一标准）已是既有的单一事实源（SSOT）：`~/.agents/skills/` 有 34 个 skill，`~/.agents/agents/search-agent.md` 1 个 agent，来源记录在 `~/.agents/.skill-lock.json`。

**问题**：同一资源在各工具目录里是散落的实体副本，改动无法同步，也无从得知哪些是受管理的、哪些是漂移的。

**目标**：在 trajectory-viewer 内新增一个 **Config Hub** 视图，把各工具的配置集中管理——统一扫描、导入、逐工具开关（软链接分发）、删除。

## 2. 核心原则

> **能通用的统一走 `~/.agents/` 软链接管理；工具不支持的，回落到各自目录的文件适配。共性用链接，特色用适配。**

由此资源分两类：

- **链接型**（skills、agents）：SSOT 目录 → 各工具目录软链接，一处改处处生效。
- **特色适配型**（MCP、settings/配置文件）：必须合并进各工具自己的配置（JSON/TOML/YAML），格式各异，不能整文件链接。

分发策略 `SyncMethod` 三档，默认 **Auto**：优先创建 symlink，失败（Windows 无开发者模式 / 权限不足）自动回退为**目录复制**。不使用 junction。

## 3. 总体架构

新增顶级视图 Config Hub，与 Session Browser 平级，入口为**顶栏切换按钮**。

Rust 侧新建 `config_hub/` 模块，与 `session_manager/` 并列，**复用其 Provider trait + 注册表模式**：

```
src-tauri/src/
├── session_manager/                # 已有：会话扫描
│   ├── provider.rs                 # SessionProvider trait + 注册表（复用此模式）
│   └── {claude,codex,dsh,opencode}.rs
└── config_hub/                     # 新增：配置管理
    ├── mod.rs                      # 资源模型 + 命令注册
    ├── registry.rs                 # ToolProvider trait + 注册表
    ├── resource.rs                 # 统一 Resource 模型
    ├── sync.rs                     # 分发引擎：软链接 + 回退复制
    ├── migrate.rs                  # 扫描 / 导入 / 迁移
    └── {claude,codex,opencode}.rs  # 每个工具的路径表与读写
```

### 3.1 ToolProvider trait

```rust
pub trait ToolProvider: Sync {
    fn id(&self) -> &'static str;
    /// 配置根目录，未安装则 None（禁用该工具）
    fn root(&self) -> Option<PathBuf>;
    /// 可被软链接的目录（skills、agents）
    fn linkable_dirs(&self) -> Vec<LinkTarget>;
    /// 系统提示词文件（第一阶段只读展示）
    fn prompt_file(&self) -> Option<PathBuf>;
    /// 其它配置文件，只读展示
    fn config_files(&self) -> Vec<ConfigFile>;
    /// MCP 配置（第一阶段只读展示）
    fn mcp_file(&self) -> Option<(PathBuf, ConfigFormat)>;
}

pub struct LinkTarget {
    pub kind: ResourceKind,   // Skill | Agent
    pub dir: PathBuf,         // 如 ~/.claude/skills
}

pub enum ConfigFormat { Json, Toml, Yaml }
```

注册表照搬 `session_manager/provider.rs::providers()` 的写法：一个 `static` 数组 + `provider(id)` 查找函数。新增工具只需实现 trait 并追加到数组。

## 4. 统一资源模型

```rust
pub enum ResourceKind { Skill, Agent, Prompt, Mcp, Config }

pub struct Resource {
    pub id: String,                              // 稳定 id，如 "skill:brainstorming"
    pub kind: ResourceKind,
    pub name: String,
    pub description: Option<String>,
    pub ssot_path: PathBuf,                      // ~/.agents/<kind>/<name>
    pub apps: BTreeMap<String, AppState>,        // 逐工具状态
}

pub enum AppState {
    Linked,   // 已链接且指向 SSOT
    Copied,   // 复制模式同步的实体副本（Auto 回退产生）
    Drifted,  // 存在但不是受管理副本（散落实体 / 被改动）
    Absent,   // 该工具没有
}
```

`Drifted` 是核心——它精确描述当前 `~/.claude/skills` 里那 8 个实体副本的状态，扫描时一眼可见哪些需要收编。

## 5. 数据流

```
扫描   四工具目录 + ~/.agents/ 汇总 → Resource 列表（含 Drifted 标记）
  ↓
导入   逐个确认 → 实体移入 ~/.agents/<kind>/<name> → 原位置建链接
  ↓
开关   toggle(id, app) → 建 / 删链接（Auto 回退复制）
  ↓
删除   SSOT 条目 + 所有链接 → 移入 ~/.trajectory-viewer/trash/（复用 trash.rs）
```

**迁移策略**：扫描后**逐个确认**。每个 `Drifted` 项展示「SSOT 是否已有 / 将覆盖还是新建 / 将建链接」预览，用户确认后才执行。全程可撤销，不自动改动。

## 6. Tauri 命令

```
config_hub_scan()                     → 全量清单（含 Drifted 标记）
config_hub_import(id, app)            → 收编单个散落副本
config_hub_toggle(id, app, enabled)   → 开关链接
config_hub_delete(id)                 → 删除（进 trash）
config_hub_config_files()             → 只读展示各工具配置文件
```

## 7. 错误处理

- **链接失败** → 自动回退复制，UI 标注 `Copied`，不静默失败。
- **权限不足** → 明确提示需开启开发者模式并给出手动指引，不反复重试。
- **删除安全** → 复用 `trash.rs`，永不物理删除；删除 SSOT 条目时先清理所有指向它的链接，避免产生孤儿链接。
- **工具未安装** → `root()` 返回 `None`，该工具整体禁用，不创建任何文件。

## 8. 测试

- **Rust**：用 `tempfile` 覆盖 `sync.rs`（链接创建/检测/漂移判定/回退复制）与 `migrate.rs`（扫描/导入/迁移）的核心路径。CI 已有 `cargo fmt --check` + `cargo clippy -D warnings`。
- **前端**：`src/utils/` 下纯逻辑用 vitest（参照现有 `layout.test.ts` / `format.test.ts`）。

## 9. 分阶段

**第一阶段（本次目标）**：skills + agents 目录型资源完整闭环——扫描 → 导入 → 统一列表 → 逐工具开关 → 删除。prompts / MCP / 配置文件仅只读展示。

**后续**：prompts 开关落地、MCP 合并写入、配置文件结构化编辑。

## 10. 已确认的决策记录

| 决策点 | 结论 |
|--------|------|
| 管理模型 | SSOT + 软链接分发 |
| 覆盖工具 | AI Code、Codex、DSH、OpenCode |
| 集成方式 | 同一 App 内新增顶级视图 |
| SSOT 位置 | 复用 `~/.agents/` |
| 迁移策略 | 扫描后逐个确认，可撤销 |
| 提示词模型 | 先导入，再按开关链接 / 直接处理（cc-switch 式） |
| 第一阶段范围 | 目录型（skills/agents）先闭环 |
| DSH agents | 暂不纳入（`.agent-presets/` 目录式，格式不通） |
| 顶栏入口 | 顶栏切换按钮 |
