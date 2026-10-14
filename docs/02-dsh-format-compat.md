# DSH 会话格式兼容说明

> 本文档记录 DSH（DeepSeek Harness）会话文件格式的版本机制、本项目
> （trajectory-viewer）的兼容现状，以及面向未来的适配层设计。
>
> 最后核实：2026-10-07。格式版本与事件清单来自 DSH 官方仓库
> `deepseek-ai/deepseek-harness` 的 schema 与文档，非推测。

---

## 1. 为什么需要这份文档

DSH 的会话文件是**带格式版本号的持久化数据**，而版本会随产品发布推进。
本项目直接解析该文件，因此版本变更会直接影响轨迹视图的正确性。

关键约束：**格式迁移是单向的**。官方提供 v0→v4 的升级链，但没有反向路径。
一旦用新版 DSH 打开旧会话，文件会被升级，旧版 CLI 将无法再读。

---

## 2. 格式版本机制

### 2.1 单一真源

- 写入器版本常量：`packages/core/session/src/types.ts` 的 `SESSION_FORMAT_VERSION`
- 官方记录：`docs/session-format-status.zh.md`

```yaml
latestFinalizedVersion: 4      # 已接受的兼容性基线
latestReleasedVersion: 3       # 已发布的产品格式（证据标签 dsh-v0.1.5-alpha.1）
```

**定稿 ≠ 发布**。V4 已定稿但官方记录仍标为未发布，而桌面端
`0.2.0-rc.2` 已经在写 v4（见下）。alpha / beta / rc 发布都会确立
已发布格式的义务——prerelease 标记不会让用户数据变成可丢弃数据。

### 2.2 版本号位置

会话文件第一行是 header，`version` 字段即格式版本：

```json
{"type":"session","version":0,"id":"session-<uuid>","createdAt":1791251521058,
 "cwd":"D:\\ds","delegationDepth":0,"agentPreset":"standard"}
```

### 2.3 版本拒绝语义

harness 读到不认识的版本会**显式拒绝**，不是静默降级：

- 更高版本 → `"the log was written by a newer harness — upgrade the harness to open it"`
- 更低版本且无升级路径 → `"...older than the supported vN, and this build ships no upgrade path for it"`

### 2.4 迁移是代际另存，不是就地覆盖

桌面端实现中 `sourcePath` 与 `currentPath` 分离，并有显式 publication 步骤
（`prepareJsonlMigration` / `prepareMigration`）。迁移链完整覆盖
v0→v1→v2→v3→v4（实测 `sessionFormatMigration` 四个 `from/to` 对齐全）。

---

## 3. 版本沿革与差异（v0 → v4）

官方格式索引（`docs/persistence-changes/historical-formats/`）：

| 格式 | 来源 | 根类型 / 类型总数 |
|---|---|---|
| 0 | `dsh-v0.1.2-rc.1` | 54 / 415 |
| 1 | PR #3349 | 54 / 415 |
| 2 | `dsh-v0.1.3-alpha.2` | 56 / 435 |
| 3 | PR #4320 | 60 / 467 |
| 4 | 当前工作树 | 62 / 590 |

### 3.1 各组件实际写入的版本

| 组件 | `SESSION_FORMAT_VERSION` | 核实方式 |
|---|---|---|
| CLI `0.1.1-rc.2`（本机全局） | **0** | 读 `dsh-session-persistence/lib/index.js` 常量 |
| 桌面端 `0.2.0-rc.2`（本机） | **4** | grep `app.asar` |
| 本机全部 49 个会话文件 | **0** | 逐个解压读 header |

> 注意：`0.1.7` 的变更日志称"Session 日志升级为 V4"，但**发布记录**与
> 实际写入器常量才是权威。本机实测 CLI 0.1.1 写的是 v0。

### 3.2 事件词汇差异

信封结构（`type` / `seq` / `time` / `data`）**各版本完全一致**，差异只在事件类型集合。

**关键边界：`assistant/chunk` 在 v2 被 `assistant/attempt` 取代**（逐版本核对官方 schema）：

| 版本 | `assistant/chunk` | `assistant/attempt` | 事件总数 |
|---|---|---|---|
| v0 | ✅ | — | 51 |
| v1 | ✅ | — | 51 |
| **v2** | **❌** | **✅** | 53 |
| v3 | ❌ | ✅ | 57 |
| v4 | ❌ | ✅ | 59 |

v2/v3/v4 的 `assistant/attempt.data` 结构**完全相同**：`{step, stream, turn}`。

**v4 相对 v0 新增 11 个事件**：

```
assistant/attempt        subagent/catalog        workspace/changes
developer/message        system/message          image/offload
deliverables/presented   tool/ptc-dispatch       tool/ptc-dispatch-start
feedback/message-put     feedback/message-delete
```

**v4 相对 v0 移除 3 个**：

```
assistant/chunk          tool/code-dispatch      tool/code-dispatch-start
```

`user/message`、`tool/call`、`tool/result`、`assistant/message` 的 `data`
结构在 v2/v3/v4 之间**逐字段一致**。

### 3.3 流式分片的去向（适配层的依据）

v2+ 把流式分片**批量折叠**进 `assistant/attempt`：

```
assistant/attempt.data.stream[] 是一个数组，元素按 type 判别：
  { type: "chunk",             time, chunk: {...} }   ← 对应 v0 的 assistant/chunk
  { type: "reasoning-chunks",  dt, index, texts, time0 }
  { type: "text-chunks",       dt, index, texts, time0 }
  { type: "tool-call-chunks",  args, dt, id, index, name, time0 }
```

其中 `chunk` 元素内的 `chunk` 对象与 v0 的
`assistant/chunk.data.chunk` **同构**，分片类型（`block-start` /
`text-delta` / `block-end` / `usage`）也一致。这就是适配层能无损还原的依据。

> `reasoning-chunks` / `text-chunks` / `tool-call-chunks` 三类无需展开：
> 归一化层对**同名顶层事件**本就是跳过的，展开只会增加开销而不改变结果。

---

## 4. 对本项目的影响

### 4.1 当前架构

```
文件 → detect_provider()          src-tauri/src/trajectory/mod.rs:85
     → parse_trajectory(id, path) src-tauri/src/trajectory/mod.rs:230
     → parser::dsh::parse_trajectory(path)   parser/dsh.rs:80
         ├─ zstd 解压                          dsh.rs:86
         ├─ 读 header.version → classify()     dsh_format.rs（适配层）
         └─ 单趟 match line_type                dsh.rs:131-558
     → Vec<TrajectoryEvent>（归一化模型）      mod.rs:41-64
     → serde camelCase → 前端
```

**关键观察**：归一化目标 `TrajectoryEvent` 已经是**版本无关的稳定契约**，
前端只消费它。版本相关的知识**全部集中在 `dsh_format.rs`**，
`dsh.rs` 的 match 只认识基线形状。

### 4.2 受影响的具体位置

| 位置 | 依赖的事件 | v2+ 下的行为（**已有适配层**） |
|---|---|---|
| `dsh.rs` `"assistant/chunk"` 分支 | `assistant/chunk` | v0/v1 命中 |
| `dsh.rs` `"assistant/attempt"` 分支 | `assistant/attempt` | v2+ 命中，经适配层还原为分片 |
| `accumulate_chunk()` | 分片 | **两条路径共用**，时序与 usage 一致 |
| `dsh.rs` 跳过的 `request/header` | 该事件 | 仍然存在，但数据被主动丢弃 |

> **`request/header` 值得注意**：实测它携带完整的 system prompt
> （46 字符～更长）与整个 tools 目录（首个请求 2 个 / 后续 32 个）。
> 本项目目前显式跳过，这是**既有取舍**，与版本升级无关，
> 但它是理解"极简注入 vs 完整注入"的关键数据源。

### 4.3 不受影响的部分

- **provider 探测**：`detect_provider()` 依据 `type == "session"` 与事件名，
  这些在各版本中依然存在，探测逻辑**无需改动**
- **`assistant/message` / `tool/call` / `tool/result` / `user/message`**：
  v2/v3/v4 的 `data` 结构逐字段一致，解析路径继续有效
- **前端全部代码**：只要 `TrajectoryEvent` 契约不变，前端零改动

### 4.4 兜底行为（适配层失效时的安全网）

时序数据缺失时并非显示为空，而是走既有兜底
（`src-tauri/src/trajectory/utils.rs:100-117`）：

- `duration_ms` ← 与下一事件的 `ts` 差值
- `ttft_ms` ← `duration_ms / 3`，上限 3000ms

有适配层时**不会走到这里**（TTFT 为精确值）。这条兜底是安全网：
即使适配层失效，v4 会话仍能渲染，只是数字退化为估算——
这也是为什么必须靠 §5.4 的回归测试来发现退化，而非等用户报错。

---

## 5. 兼容转换层（已实现）

### 5.1 分层结构

```
① 原始行（zstd JSONL）
       ↓
② 格式适配层   src-tauri/src/trajectory/parser/dsh_format.rs
   读 header.version；把 v2+ 的 assistant/attempt 还原为分片
       ↓
③ 归一化层     dsh.rs 的 match（不变）
   基线形状 → TrajectoryEvent
       ↓
④ 输出/前端    （不变）
```

核心：**适配层把新版本降到基线形状**（v0/v1），归一化层继续只认识一种输入。

### 5.2 适配层做了什么

`dsh_format::attempt_chunks(&Value)` 从 `assistant/attempt` 中提取
`stream[]` 里 `type == "chunk"` 的条目，连同 hoist 出来的 `turn` / `step`
和每条自己的 `time`。解析器侧新增一个 `assistant/attempt` 分支，
把结果喂给**与 v0 路径共用的** `accumulate_chunk()`。

版本处理：

```rust
pub const BASELINE_VERSION: u64 = 1;   // 归一化层写死的基线
pub const MAX_KNOWN_VERSION: u64 = 4;  // 本适配层认识的最高版本

classify(v) →
  v <= 1  → Native            // 直接读
  v <= 4  → Lowered(v)        // 还原分片
  v >  4  → Unknown(v)        // 尽力解析 + 显式告警
```

未知版本**不静默处理**：解析照常进行（尽力而为），但会输出

```
[trajectory] <path>: session log format v5 is newer than this build
             understands (max v4); timeline may be incomplete
```

### 5.3 验证（实测，非推测）

用**同一份真实会话**（`session-64f5ea74`，24186 行 / 3551 个
`assistant/chunk`）合成等价的 v4 样本（折叠为 1919 个 `assistant/attempt`），
经真实 `parse_trajectory` 对比：

| 指标 | v0 样本 | v4 样本（有适配层） | v4 样本（停用适配层） |
|---|---|---|---|
| events | 408 | 408 | 408 |
| assistant-message | 136 | 136 | 136 |
| 有精确 TTFT | 136 | **136** | 133 |
| 有精确 duration | 407 | **407** | 404 |
| input tokens | 192762 | **192762** | 192762 |
| output tokens | 851100 | **851100** | 851100 |
| TTFT 前 6 值 | `[115427, 933, …]` | **同左，逐值相同** | `[2, 0, 0, 1, 1, 3]` |

**有适配层时 v0 与 v4 逐值一致**；停用后 TTFT 从 `115427ms` 退化为 `2ms`
——这正是"静默错误"的形态：不报错，但数字全错。

### 5.4 回归保护

两条常驻测试（非临时探针）锁定该行为：

- `dsh_format::tests`（10 条）：版本分类、分片提取、畸形条目丢弃、
  非 chunk 载体忽略、时间回退
- `dsh::tests::v0_and_v2_shapes_parse_identically`：同一会话的两种写法
  必须解析出**完全相同**的 ts / duration / ttft / usage
- `dsh::tests::newer_versions_still_recover_streamed_chunks`：v4 头
  仍能还原分片时序

### 5.5 为什么"其他层不动"

| 层 | 是否改动 |
|---|---|
| 前端（`src/**`） | **否**——契约不变 |
| `trajectory/mod.rs` 分发与 `TrajectoryEvent` | **否** |
| 其他 provider（claude / codex / opencode） | **否** |
| `dsh.rs` 归一化 match | 仅新增一个分支 + 抽出 `accumulate_chunk()` |
| **新增** `parser/dsh_format.rs` | 是 |

### 5.6 尚未覆盖的部分

- **v4 新增的 11 个事件**（`workspace/changes`、`image/offload`、
  `system/message`、`subagent/catalog` 等）目前仍走 `_ => {}` 忽略分支。
  它们不影响既有时间线的正确性，但对应信息在视图中缺失。
  需要真实 v4 会话来确认语义后再补。
- **`request/header`**（system prompt + tools 快照）是既有取舍，非版本问题。

---

## 6. 升级检查清单

### 升级前

- [ ] 备份 `~/.dsh/profiles`
- [ ] **保留一份 v0 会话副本**——迁移单向，升级后旧 CLI 读不了新格式
- [ ] 确认目标版本（建议锁定 `0.2.0-rc.2`，避免 `0.2.1-alpha.1` 的
      invariant 移除破坏 `dsh-better-sidebar`）

### 升级后

- [ ] 用新版生成一个 v4 会话，跑 `parse_trajectory`，确认渲染无异常
- [ ] **核对 TTFT 是否为精确值**——适配层生效时应等于流式首 token 间隔；
      若看到个位数毫秒（如 `2`），说明适配层没接上，退化成了估算
- [ ] 检查 `tool/call` 与 `tool/result` 配对是否完整
- [ ] 若日志出现 `newer than this build understands` 告警，
      说明格式已超出 `MAX_KNOWN_VERSION`，需要扩展适配层

### 验证命令

```bash
# 看会话格式版本（需要 zstd 解压）
uv run --with zstandard python -c "
import zstandard,json,sys
d=zstandard.ZstdDecompressor()
print(json.loads(d.stream_reader(open(sys.argv[1],'rb')).read().decode().splitlines()[0])['version'])
" <session.jsonl.zstd>
```

---

## 7. 相关源码位置

**本项目**

```
src-tauri/src/trajectory/mod.rs:85          detect_provider()
src-tauri/src/trajectory/mod.rs:230         parse_trajectory() 分发
src-tauri/src/trajectory/mod.rs:41-64       TrajectoryEvent（稳定契约）
src-tauri/src/trajectory/parser/dsh_format.rs   ★ 格式适配层（版本知识集中处）
src-tauri/src/trajectory/parser/dsh.rs      入口 + 归一化 match
src-tauri/src/trajectory/parser/dsh.rs      accumulate_chunk()（v0 与 v2+ 共用）
src-tauri/src/trajectory/utils.rs:100-117   时序兜底
```

**DSH 官方（参考）**

```
docs/session-format-status.zh.md                      版本真源
docs/persistence-schema.json                          v4 完整 schema
docs/persistence-changes/historical-formats/vN.zh.md  各版本文档
docs/persistence-changes/finalized/v4.json            v4 检查点
```
