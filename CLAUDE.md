# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Development
pnpm tauri dev          # Start Tauri dev server (Rust + Vite hot-reload)
pnpm dev:web            # Start Vite dev server only (frontend)

# Build
pnpm build:web          # Build frontend only (Vite)
pnpm build              # Full Tauri production build

# Rust
cd src-tauri && cargo check       # Check Rust compilation
cd src-tauri && cargo test        # Run all Rust tests
cd src-tauri && cargo test dsh    # Run DSH parser tests only
cd src-tauri && cargo test claude # Run Claude parser tests only

# TypeScript
npx tsc --noEmit        # Type check without emitting files
```

## Architecture

### Data Flow

```
JSONL/Zstd file → Rust parser → TrajectoryData → Tauri invoke → React components
```

### Project Structure

```
trajectory-viewer/
├── src-tauri/                    # Rust backend (Tauri v2)
│   └── src/
│       ├── main.rs               # Binary entry point
│       ├── lib.rs                # Tauri app builder + command registration
│       ├── commands.rs           # Tauri commands (4 total)
│       ├── session_manager.rs    # Session scanning + message loading (4 providers)
│       └── trajectory/
│           ├── mod.rs            # Data model (TrajectoryData, TrajectoryEvent, ContentBlock)
│           │                     # + provider detection (claude/codex/dsh) + parse dispatch
│           ├── utils.rs          # Shared: timestamp parsing, timing estimation, file head/tail reading
│           └── parser/
│               ├── mod.rs        # Parser module declarations
│               ├── claude.rs     # Claude Code JSONL parser (5 tests)
│               ├── codex.rs      # Codex JSONL parser (4 tests)
│               ├── dsh.rs        # DSH zstd-compressed JSONL parser (3 tests)
│               └── opencode.rs   # OpenCode JSON files + SQLite parser (5 tests)
│
├── src/                          # Frontend (React + TypeScript + Tailwind)
│   ├── main.tsx                  # React entry point
│   ├── App.tsx                   # Root: session browser ↔ standalone file view
│   ├── api.ts                    # Tauri invoke wrapper (4 API calls)
│   ├── types.ts                  # TrajectoryData, SessionMeta, SessionMessage interfaces
│   ├── styles.css                # Tailwind base + CSS variables (light/dark)
│   ├── lib/utils.ts              # cn() classname merge utility
│   ├── utils/
│   │   ├── layout.ts             # deriveTrajectoryLayout() — event→turn-grouped layout
│   │   └── format.ts             # Time/token formatting utilities
│   └── components/
│       ├── SessionBrowser.tsx     # Main view: sidebar + messages/trajectory tabs
│       ├── TrajectoryView.tsx     # Orchestrator: toolbar + timeline + table
│       ├── TrajectoryToolbar.tsx  # Search, collapse toggles, duration switch
│       ├── TrajectoryTimeline.tsx # 3-lane Chrome-Network-style timeline
│       ├── TrajectoryTable.tsx    # Virtual-scrolled event table + detail panel
│       ├── TrajectoryCell.tsx     # Single row renderer (7 kinds with icons)
│       ├── TrajectoryDetail.tsx   # Detail panel (5 tabs: Summary/Payload/Result/Timing/Usage)
│       └── FileDropZone.tsx       # File picker landing page
```

### Session Providers

| Provider | Format | Storage Path | Detection |
|----------|--------|-------------|-----------|
| Claude Code | Plain JSONL | `~/.claude/projects/{project}/{sessionId}.jsonl` | `sessionId` field or `message.role` |
| Codex | Plain JSONL | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` | `type: "response_item"` |
| DSH | Zstd-compressed JSONL | `~/.dsh/sessions/{project}/{sessionId}/session.jsonl.zstd` | `type: "session"` or `"user/message"` |
| OpenCode | JSON files + SQLite | `~/.local/share/opencode/{storage,opencode.db}` | session file `id: "ses_*"` or `sqlite:` ref |

### Key Design Decisions

- **No react-query**: Trajectory data is fetched via direct `invoke()` calls instead of react-query to minimize dependencies
- **Virtual scrolling**: `@tanstack/react-virtual` for efficient rendering of large event lists
- **Resizable panels**: Both sidebar and detail panel support drag-to-resize via `mousedown`/`mousemove` handlers
- **Provider detection**: `detect_provider()` reads first 200 lines (or decompresses zstd header) to identify format
- **Turn/step derivation**: Events with missing turn/step get derived values (user-message starts new turn, tool-call → step 1)
- **Timing estimation**: Duration estimated from next-event timestamp delta; TTFT ≈ duration/3 (capped at 3s) when not native

### Adding a New Provider Parser

1. Create `src-tauri/src/trajectory/parser/{name}.rs` with `parse_trajectory(path) -> Result<(String, Vec<TrajectoryEvent>), String>`
2. Add `pub mod {name};` to `src-tauri/src/trajectory/parser/mod.rs`
3. Add detection to `detect_provider()` in `src-tauri/src/trajectory/mod.rs`
4. Add route in `parse_trajectory()` in `src-tauri/src/trajectory/mod.rs`
5. Add session scanning + message loading in `src-tauri/src/session_manager.rs`
6. Add provider icon + filter button in `src/components/SessionBrowser.tsx`