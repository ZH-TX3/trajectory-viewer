# DSH Trajectory Viewer — 需求文档

## 1. 概述

**Trajectory Viewer** 是一个独立的桌面应用，用于将 AI 会话日志（JSONL 文件）解析为结构化时间线，展示用户与 AI 助手之间每一轮交互的完整记录。

> 从 CC Switch 项目中独立提取，聚焦轨迹功能，移除所有无关干扰代码。

### 目标

- 左侧会话浏览器：自动扫描 Claude Code / Codex / DSH 会话，按项目目录层级分组，支持 Provider 过滤
- 右侧双标签页：Messages（对话记录）与 Trajectory（轨迹视图）切换
- 支持拖入/选择 JSONL 文件，自动识别 provider 类型
- 解析为结构化时间线，展示完整的交互轮次
- 提供搜索、折叠、时间线范围选择、详情检视等功能
- 支持海量事件（虚拟滚动）

---

## 2. 数据模型

### 2.1 TrajectoryData（统一轨迹数据）

```typescript
interface TrajectoryData {
  sessionId: string;
  providerId: string;
  events: TrajectoryEvent[];
  metadata: TrajectoryMetadata;
}
```

### 2.2 TrajectoryMetadata（会话元数据）

```typescript
interface TrajectoryMetadata {
  model: string | null;
  totalInputTokens: number | null;
  totalOutputTokens: number | null;
  totalDurationMs: number | null;
  eventCount: number;
}
```

### 2.3 TrajectoryEvent（轨迹事件）

| 字段 | 类型 | 说明 |
|------|------|------|
| `seq` | `number` | 序号 |
| `ts` | `number` | 时间戳（毫秒） |
| `eventType` | `string` | 事件类型（6 种） |
| `role` | `string \| null` | user / assistant / tool |
| `content` | `string \| null` | 文本内容 |
| `contentBlocks` | `ContentBlock[] \| null` | 结构化内容块 |
| `toolCallId` | `string \| null` | 工具调用 ID |
| `toolName` | `string \| null` | 工具名称 |
| `toolArgs` | `string \| null` | 工具参数（JSON） |
| `toolResult` | `string \| null` | 工具执行结果 |
| `isError` | `boolean \| null` | 是否错误 |
| `turn` | `number \| null` | 轮次编号 |
| `step` | `number \| null` | 步骤编号 |
| `durationMs` | `number \| null` | 耗时 |
| `ttftMs` | `number \| null` | 首 Token 时间 |
| `inputTokens` | `number \| null` | 输入 Token |
| `outputTokens` | `number \| null` | 输出 Token |
| `reasoningTokens` | `number \| null` | 推理 Token |
| `cacheReadTokens` | `number \| null` | 缓存读取 Token |
| `cacheWriteTokens` | `number \| null` | 缓存写入 Token |
| `model` | `string \| null` | 模型名称 |
| `provider` | `string \| null` | 供应商标识 |

### 2.4 事件类型（6 种）

| 事件类型 | 说明 |
|----------|------|
| `user-message` | 用户输入的文本消息 |
| `assistant-message` | 助手的回复消息 |
| `tool-call` | 工具调用（含调用ID、工具名、参数） |
| `tool-result` | 工具执行结果 |
| `turn-boundary` | 轮次分隔标记 |
| `compaction` | 上下文压缩记录 |

### 2.5 ContentBlock（内容块）

```typescript
interface ContentBlock {
  blockType: string;   // text / reasoning / tool-call / tool-result / image
  text: string | null;
  toolCallId: string | null;
  toolName: string | null;
  toolArgs: string | null;
  imageSrc: string | null;
}
```

---

## 3. 支持格式

### 3.1 Provider 解析器

| Provider | 解析方式 | 状态 |
|----------|----------|------|
| Claude Code | JSONL 逐行解析 `type` 字段：`user` / `assistant` / `tool_call` / `tool_result` / `tool_use` | ✅ 完成 |
| Codex | JSONL 解析 `type: "response_item"`，处理 `payload.type`：`message` / `function_call` / `function_call_output` | ✅ 完成 |
| DSH | Zstd 压缩 JSONL，解析 `user/message` / `assistant/message` / `tool/call` / `tool/result` 事件 | ✅ 完成 |
| Gemini | TBD | ❌ 未实现 |
| OpenCode | TBD（SQLite 存储） | ❌ 未实现 |
| OpenClaw | TBD | ❌ 未实现 |
| Hermes | TBD（SQLite 存储） | ❌ 未实现 |

### 3.2 自动检测（Provider Detection）

读取 JSONL 前 200 行，根据特征识别（zstd 文件则先解压头部）：

- 文件扩展名为 `.zstd` / `.zst` → 尝试解压头部判断 DSH
- 出现 `type: "session"` / `"user/message"` / `"assistant/message"` → DSH
- 出现 `sessionId` 字段 → Claude
- 出现 `message.role` 字段 → Claude
- 出现 `type: "user"` / `"assistant"` / `"tool_call"` / `"tool_result"` → Claude
- 出现 `type: "response_item"` → Codex
- 出现 `type: "session_meta"` + `payload.id` → Codex

---

## 4. 布局算法

### 4.1 deriveTrajectoryLayout

1. 遍历 `TrajectoryEvent[]`，将每个事件转为 `TrajectoryCellProps`
2. 按 `turn` 字段分组（turn 0 为独立序章 "Prologue"）
3. 每个 turn 内按 `step` 进一步分组为 `TrajectoryGroupModel`
4. 输出 `TrajectoryTurnModel[]` 供 UI 渲染

### 4.2 输出结构

```typescript
interface TrajectoryTurnModel {
  turn: number | null;
  groups: TrajectoryGroupModel[];
}

interface TrajectoryGroupModel {
  title: string;        // "Message" | "Step 1" | "Step 2" | ...
  description?: string;
  cells: TrajectoryCellProps[];
}
```

### 4.3 Cell 类型（7 种）

| Cell Kind | 来源事件 | 说明 |
|-----------|---------|------|
| `system` | 系统消息 | 系统提示/配置 |
| `user` | `user-message` | 用户输入 |
| `context` | 上下文消息 | 环境上下文 |
| `compacted` | `compaction` | 上下文压缩 |
| `message` | `assistant-message` | 助手回复 |
| `tool` | `tool-call` / `tool-result` | 工具调用或结果 |
| `subtool` | 子工具 | 嵌套工具调用 |

### 4.4 Cell 属性

```typescript
interface TrajectoryCellProps {
  index: number;
  kind: TrajectoryCellKind;
  recordId?: string;
  text: string;
  previewMarkdown?: string;
  opensTurn?: boolean;
  sourceSeq?: number;
  requestOnly?: boolean;

  // 完整内容（详情面板用）
  inputDetail?: string;
  outputDetail?: string;
  thinkingDetail?: string;
  sourceBlocks?: TrajectorySourceBlock[];
  outputBlocks?: TrajectorySourceBlock[];

  // 工具相关
  callId?: string;
  toolName?: string;
  toolArgs?: string;
  schemaDetail?: string;
  isError?: boolean;
  result?: string;
  resultPreviewMarkdown?: string;

  // 时间
  timeSeconds: number | null;
  startedAt?: number | null;

  // Token 用量
  input?: number;
  output?: number;
  think?: number;
  cacheRead?: number;
  cacheWrite?: number;

  // 助手特有指标
  assistantMetrics?: {
    timingRecorded: boolean;
    stepStartTime: number | null;
    firstTokenTime: number | null;
    completedTime: number | null;
    usageProvided: boolean;
    outputTokens: number | null;
  };

  selected?: boolean;
}
```

---

## 5. UI 组件体系

### 5.1 组件树

```
App
 └── TrajectoryView (主容器)
      ├── TrajectoryToolbar (工具栏)
      ├── TrajectoryTimeline (时间线概览)
      └── TrajectoryTable (记录表 + 虚拟滚动)
           ├── TrajectoryCell (单行记录)
           └── TrajectoryDetail (详情检查面板)
```

### 5.2 TrajectoryView（主容器）

- 接收 `providerId` + `sourcePath`
- 调用数据获取 hook 获取后端数据
- 组装 Toolbar + Timeline + Table 三大部分
- 管理全局状态：搜索、折叠、duration 切换、timeline 范围选择
- 加载态和错误处理

### 5.3 TrajectoryToolbar（工具栏）

| 功能 | 说明 |
|------|------|
| 搜索框 | 实时过滤匹配行，高亮显示匹配结果 |
| 折叠全部轮次 | Collapse All Turns / Expand All Turns |
| 折叠全部助手消息 | Collapse All Assistants / Expand All Assistants |
| Duration 切换 | 切换实际耗时 / 相对耗时显示 |

### 5.4 TrajectoryTimeline（时间线概览）

- Chrome Network 风格横向时间线
- 三车道：Input / Model / Tools
- 每一行代表一个事件，按时间排列
- 鼠标悬停 → 显示详情 tooltip（事件名、耗时、TTFT）
- 范围选择 → 拖拽选择时间范围，Table 联动聚焦
- 不同颜色区分 user / assistant / tool 事件

### 5.5 TrajectoryTable（记录表 + 虚拟滚动）

- 使用虚拟滚动（`@tanstack/react-virtual`）支持海量事件
- 每行展示：kind 图标 + 颜色标签 + 文本摘要 + 耗时条
- 行点击 → 打开详情面板
- 搜索匹配高亮
- 轮次折叠 → 折叠后显示摘要行
- 助手消息折叠 → 折叠后只显示首条

### 5.6 TrajectoryDetail（详情检查面板）

5 个标签页：

| 标签 | 内容 |
|------|------|
| **Summary** | 概览：事件类型、角色、时间、耗时 |
| **Payload** | 完整消息内容 / 工具参数（JSON 格式化） |
| **Result** | 工具执行结果（仅 tool-result 类型） |
| **Timing** | 耗时分析：TTFT、总耗时、各阶段耗时 |
| **Usage** | Token 用量：input / output / cache-read / cache-write |

### 5.7 TrajectoryCell（单行记录）

- 7 种 kind 类型，各有对应的 SVG 图标、颜色、背景样式
- 展示：序号、类型标签、文本摘要、工具名 badge、耗时

---

## 6. 数据流

```
JSONL 文件
  ↓
Rust 解析器（按 provider 分发）
  ↓ claude.rs / codex.rs / dsh.rs
TrajectoryData
  ↓ Tauri invoke
前端数据获取
  ↓
deriveTrajectoryLayout()
  ↓
TrajectoryTurnModel[]
  ↓
Toolbar + Timeline + Table + Detail
```

---

## 7. 技术栈

| 层 | 技术选型 |
|----|----------|
| 桌面框架 | **Tauri v2** |
| 前端 | **React 18** + **TypeScript** |
| 样式 | **Tailwind CSS** + **shadcn/ui** |
| 虚拟滚动 | **@tanstack/react-virtual** |
| 数据获取 | 直接 `invoke()` 调用（无需 react-query） |
| 路由 | 无需路由库，`useState<View>` 切换 |

---

## 8. 文件结构

```
trajectory-viewer/
├── src-tauri/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── lib.rs                    ← 应用入口 + 命令注册
│       ├── commands.rs               ← Tauri 命令
│       ├── session_manager.rs        ← 会话扫描 + 消息加载（Claude/Codex/DSH）
│       └── trajectory/
│           ├── mod.rs                ← 数据模型 + 自动检测 + 解析分发
│           ├── utils.rs              ← 共享工具函数
│           └── parser/
│               ├── mod.rs            ← 解析器模块声明
│               ├── claude.rs         ← Claude Code 解析器
│               ├── codex.rs          ← Codex 解析器
│               └── dsh.rs            ← DSH zstd 解析器
├── src/
│   ├── main.tsx                      ← 入口
│   ├── App.tsx                       ← 根组件（会话浏览 ⇄ 独立文件切换）
│   ├── types.ts                      ← 类型定义
│   ├── api.ts                        ← Tauri invoke 封装
│   ├── styles.css                    ← Tailwind + CSS 变量 + timeline-focus 置灰
│   ├── lib/utils.ts                  ← cn() 类名合并
│   ├── components/
│   │   ├── SessionBrowser.tsx        ← 主视图：会话列表 + 双标签页
│   │   ├── FileDropZone.tsx          ← 文件拖入/选择
│   │   ├── TrajectoryView.tsx        ← 轨迹编排器（状态管理 + focus 计算）
│   │   ├── TrajectoryToolbar.tsx     ← 工具栏
│   │   ├── TrajectoryTimeline.tsx    ← 三车道时间线（拖拽范围选择）
│   │   ├── TrajectoryTable.tsx       ← 记录表（虚拟滚动 + 焦点置灰）
│   │   ├── TrajectoryCell.tsx        ← 单行记录
│   │   └── TrajectoryDetail.tsx      ← 详情面板（可拖拽宽度）
│   └── utils/
│       ├── layout.ts                 ← 布局算法 + 时间轴模型 + focus 计算
│       └── format.ts                 ← 格式化工具
├── docs/
│   └── 01-requirements.md           ← 本文档
├── package.json
├── tsconfig.json
├── vite.config.ts
├── tailwind.config.cjs
├── postcss.config.cjs
└── index.html
```

---

## 9. 实施路线图

### Phase 1: 项目骨架搭建
- [x] 创建 Tauri v2 项目
- [x] 配置 Tailwind CSS + shadcn/ui
- [x] 定义 TypeScript 类型
- [x] 实现 Rust 数据模型

### Phase 2: Rust 后端解析器
- [x] 实现 Claude Code 解析器
- [x] 实现 Codex 解析器
- [x] 实现 DSH（zstd）解析器
- [x] 实现 Provider 自动检测
- [x] 实现 Tauri 命令

### Phase 2.5: 会话系统
- [x] 实现会话扫描（Claude/Codex/DSH 默认路径）
- [x] 实现消息加载（Messages 标签页）
- [x] 实现项目目录层级分组 + Provider 过滤

### Phase 3: 前端布局算法
- [x] 实现 `deriveTrajectoryLayout()`
- [x] 实现格式化工具函数
- [x] 实现 `deriveTrajectoryTimeline()`（统一时间轴模型）
- [x] 实现 `trajectoryTimelineFocusIndexes()`（时间范围 → 记录索引）

### Phase 4: UI 组件
- [x] 实现 FileDropZone
- [x] 实现 SessionBrowser（会话列表 + 双标签页）
- [x] 实现 TrajectoryView
- [x] 实现 TrajectoryToolbar
- [x] 实现 TrajectoryTimeline（拖拽范围选择，DSH 参考实现）
- [x] 实现 TrajectoryTable（虚拟滚动 + focus 置灰）
- [x] 实现 TrajectoryCell
- [x] 实现 TrajectoryDetail（可拖拽宽度 + 蓝色高亮标签页）

### Phase 5: 集成测试
- [x] Rust 单元测试（12 个）
- [ ] 前端组件测试
- [x] 端到端验证

---

## 10. 与 CC Switch 的差异

| 方面 | CC Switch | Trajectory Viewer |
|------|-----------|-------------------|
| 范围 | 配置管理 + 代理 + 会话管理 + 轨迹 | 仅轨迹查看 |
| 入口 | SessionManagerPage 内嵌 | 独立文件选择/拖入 |
| 数据获取 | react-query | 直接 invoke |
| 路由 | 多页面 | 单页面（useState 切换） |
| i18n | 多语言 | 英文（可扩展） |
| Provider 管理 | 完整 CRUD | 无（仅解析） |