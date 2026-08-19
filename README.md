# Trajectory Viewer

一个独立的桌面应用，用于将 AI 会话日志（JSONL 文件）解析为结构化时间线，展示用户与 AI 助手之间每一轮交互的完整记录。

A standalone desktop app that parses AI session logs (JSONL) into a structured timeline — the complete record of every turn between a user and an AI assistant.

> 独立于 CC Switch，聚焦轨迹（Trajectory）功能。时间轴与记录表的联动逻辑移植自 DSH `@deepseek-ai/dsh-client-ui-trajectory` 的参考实现。

---

## ✨ 功能特性

- **会话浏览** — 左侧边栏展示 Claude Code / Codex / DSH 会话，按项目目录层级分组，支持 Provider 过滤
- **对话记录** — 右侧 Messages 标签页展示每个会话的角色消息流
- **轨迹视图** — Trajectory 标签页展示完整的交互轨迹：
  - **时间线（Timeline）** — Chrome Network 风格三车道缩略图，拖拽选择时间范围
  - **记录表（Table）** — 虚拟滚动支持海量事件，行点击打开详情面板
  - **详情面板（Detail）** — 5 个标签页：Summary / Payload / Result / Timing / Usage
- **时间范围联动** — 时间轴上选中一段范围 → 表格中范围外的行自动置灰，并滚动定位到选中第一条记录
- **搜索** — 实时过滤匹配行并高亮
- **折叠** — 折叠全部轮次 / 折叠全部助手消息
- **可拖拽分栏** — 左侧会话栏与右侧详情面板均可左右拖动调整宽度
- **独立文件查看** — 打开任意 JSONL 文件自动识别 Provider

---

## 🎯 支持的会话格式

| Provider | 格式 | 存储路径 | 说明 |
|----------|------|---------|------|
| **Claude Code** | 普通 JSONL | `~/.claude/projects/{project}/{session}.jsonl` | `user` / `assistant` / `tool_use` / `tool_result` 事件 |
| **Codex** | 普通 JSONL | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` | `response_item` + `payload.type` 事件 |
| **DSH** | Zstd 压缩 JSONL | `~/.dsh/sessions/{project}/{session}/session.jsonl.zstd` | `user/message` / `assistant/message` / `tool/call` / `tool/result` 事件 |

每个 Provider 的 JSONL 会自动检测识别（读取前 200 行特征）。

---

## 🛠️ 技术栈

| 层 | 技术选型 |
|----|----------|
| 桌面框架 | **Tauri v2** |
| 前端 | **React 18** + **TypeScript** |
| 样式 | **Tailwind CSS** + CSS 变量（明暗主题） |
| 虚拟滚动 | **@tanstack/react-virtual** |
| 图标 | **lucide-react** |
| 后端压缩处理 | **zstd**（DSH 会话） |

---

## 🚀 快速开始

### 环境要求

- Node.js ≥ 18
- Rust ≥ 1.77
- pnpm ≥ 8

### 开发

```bash
# 安装依赖
pnpm install

# 启动 Tauri 开发（Rust + Vite 热更新）
pnpm tauri dev

# 仅启动 Vite 前端（调试 UI）
pnpm dev:web
```

### 构建

```bash
# 构建前端
pnpm build:web

# 完整 Tauri 生产构建
pnpm build
```

### 测试

```bash
# Rust 单元测试（Claude / Codex / DSH 解析器）
cd src-tauri && cargo test

# 运行单个解析器测试
cd src-tauri && cargo test dsh
cd src-tauri && cargo test claude
cd src-tauri && cargo test codex

# TypeScript 类型检查
npx tsc --noEmit
```

---

## 🗂️ 目录结构

```
trajectory-viewer/
├── src-tauri/                    # Rust 后端（Tauri v2）
│   └── src/
│       ├── main.rs               # 二进制入口
│       ├── lib.rs                # Tauri 构建器 + 命令注册
│       ├── commands.rs           # Tauri 命令（4 个）
│       ├── session_manager.rs    # 会话扫描 + 消息加载（3 个 Provider）
│       └── trajectory/
│           ├── mod.rs            # 数据模型 + Provider 检测 + 解析分发
│           ├── utils.rs          # 时间戳解析 / timing 估算 / 文件读取
│           └── parser/
│               ├── claude.rs     # Claude Code 解析器
│               ├── codex.rs      # Codex 解析器
│               └── dsh.rs        # DSH zstd 解析器
│
├── src/                          # 前端（React + TypeScript + Tailwind）
│   ├── main.tsx                  # React 入口
│   ├── App.tsx                   # 根组件（会话浏览 ⇄ 独立文件切换）
│   ├── api.ts                    # Tauri invoke 封装
│   ├── types.ts                  # TrajectoryData / SessionMeta 等类型
│   ├── styles.css                # Tailwind + CSS 变量 + 置灰规则
│   ├── lib/utils.ts              # cn() 类名合并
│   ├── utils/
│   │   ├── layout.ts             # deriveTrajectoryLayout + 时间轴模型 + focus 计算
│   │   └── format.ts             # 格式化工具
│   └── components/
│       ├── SessionBrowser.tsx    # 主视图：会话列表 + 双标签页
│       ├── TrajectoryView.tsx    # 轨迹编排器（Toolbar + Timeline + Table）
│       ├── TrajectoryToolbar.tsx # 搜索 / 折叠 / Duration 切换
│       ├── TrajectoryTimeline.tsx# 三车道时间线（拖拽范围选择）
│       ├── TrajectoryTable.tsx   # 虚拟滚动记录表 + 详情面板
│       ├── TrajectoryCell.tsx    # 单行渲染（7 种 kind）
│       ├── TrajectoryDetail.tsx  # 详情面板（5 标签页）
│       └── FileDropZone.tsx      # 文件选择页
└── docs/
    └── 01-requirements.md        # 需求文档
```

---

## 🏗️ 架构总览

### 数据流

```
JSONL / Zstd 文件
  ↓
Rust 解析器（按 Provider 分发）
  ↓ claude.rs / codex.rs / dsh.rs
TrajectoryData
  ↓ Tauri invoke
前端 api.ts
  ↓
deriveTrajectoryLayout()      → TrajectoryTurnModel[]
deriveTrajectoryTimeline()    → TimelineModel（坐标空间）
  ↓
TrajectoryView（Toolbar + Timeline + Table + Detail）
```

### 时间轴范围选择（DSH 参考实现）

轨迹功能的核心联动逻辑，移植自 DSH：

1. **`deriveTrajectoryTimeline(turns, mode)`** 将每个记录投影为坐标空间中的 span
   - `sequence` 模式 → 按记录序数（0..N）
   - `actual` 模式 → 按真实时间戳
2. 时间轴拖拽产生一个该坐标空间的 `range`
3. **`trajectoryTimelineFocusIndexes()`** 用 span 区间重叠（`span.start <= range.end && span.end >= range.start`）计算精确的记录索引集合
4. 表格中不在集合内的行设置 `data-timeline-focus="outside"` → CSS `opacity:.25` 置灰

这种设计让时间轴绘制与表格置灰共用**同一坐标系**，避免漂移。

---

## 📝 文档

- [需求文档](docs/01-requirements.md) — 数据模型、布局算法、UI 组件体系、实施路线图
- [CLAUDE.md](CLAUDE.md) — 供 Claude Code 使用的项目指引

---

## 📄 许可证

[MIT](LICENSE)