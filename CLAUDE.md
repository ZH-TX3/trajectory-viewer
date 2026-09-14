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
│       ├── commands.rs           # Tauri commands
│       ├── session_manager/      # Session discovery per tool
│       │   ├── mod.rs            # Shared types, dispatch, delete/trash policy
│       │   ├── provider.rs       # SessionProvider trait + registry
│       │   ├── utils.rs          # Shared path/text helpers
│       │   └── {claude,codex,dsh,opencode}.rs
│       ├── backup.rs             # Session dirs → zip backups (+ auto interval/retention)
│       ├── trash.rs              # Restorable deletes (~/.trajectory-viewer/trash/)
│       ├── export.rs             # Session → Markdown / JSONL
│       ├── config_hub/           # Unified config management (skills/agents/prompts/MCP)
│       │   ├── mod.rs            # Hub snapshot + Tauri commands + id parsing
│       │   ├── registry.rs       # ToolProvider trait + registry
│       │   ├── resource.rs       # Resource model + AppState
│       │   ├── scan.rs           # SSOT + tool dirs → merged Resource list
│       │   ├── sync.rs           # Link/copy engine (SyncMethod::Auto fallback)
│       │   ├── migrate.rs        # Import / undo / delete (into trash)
│       │   ├── state.rs          # Copy markers (~/.trajectory-viewer/config-hub.json)
│       │   ├── utils.rs          # SSOT paths, entry listing, descriptions
│       │   └── {claude,codex,dsh,opencode}.rs
│       └── trajectory/
│           ├── mod.rs            # Data model + provider detection + parse dispatch
│           ├── utils.rs          # Timestamps, timing, head/tail reads, ParseWarnings
│           └── parser/
│               ├── mod.rs        # Parser module declarations
│               ├── claude.rs     # AI Code JSONL parser
│               ├── codex.rs      # Codex JSONL parser
│               ├── dsh.rs        # DSH zstd-compressed JSONL parser
│               └── opencode.rs   # OpenCode JSON files + SQLite parser
│
├── src/                          # Frontend (React + TypeScript + Tailwind)
│   ├── main.tsx                  # React entry point
│   ├── App.tsx                   # Root: sessions ↔ config hub ↔ settings ↔ file view
│   ├── api.ts                    # Tauri invoke wrapper
│   ├── types.ts                  # Shared interfaces
│   ├── styles.css                # Tailwind base + CSS variables (light/dark)
│   ├── lib/utils.ts              # cn() classname merge utility
│   ├── utils/
│   │   ├── layout.ts             # deriveTrajectoryLayout() — event→turn-grouped layout
│   │   ├── format.ts             # Time/token formatting utilities
│   │   ├── configHub.ts          # Resource filtering + state labels
│   │   └── {layout,format,configHub}.test.ts  # Vitest unit tests
│   └── components/
│       ├── SessionBrowser.tsx     # Main view: sidebar + messages/trajectory tabs
│       ├── config-hub/            # Config Hub view
│       │   ├── ConfigHubView.tsx      # Orchestrator: filters + list + read-only config
│       │   ├── ResourceList.tsx       # Resource rows with per-tool switches
│       │   ├── AppToggleGroup.tsx     # Per-tool link/copy switches
│       │   ├── ImportPreviewDialog.tsx  # Import preview + conflict choice
│       │   ├── DeleteConfirmDialog.tsx  # Delete → trash confirmation
│       │   └── ConfigReadonlyPanel.tsx  # Read-only prompts/config files
│       ├── SettingsView.tsx       # Settings (General / Advanced tabs)
│       ├── BackupSection.tsx      # Backup & restore panel (Advanced tab)
│       ├── TrashSection.tsx       # Trash list with restore / purge
│       ├── TrajectoryView.tsx     # Orchestrator: toolbar + timeline + table
│       ├── TrajectoryErrorBoundary.tsx  # Shows render crashes on-page
│       ├── TrajectoryToolbar.tsx  # Search, collapse toggles, duration switch
│       ├── TrajectoryTimeline.tsx # 3-lane Chrome-Network-style timeline
│       ├── TrajectoryTable.tsx    # Virtual-scrolled event table + detail panel
│       ├── TrajectoryCell.tsx     # Single row renderer (7 kinds with icons)
│       ├── TrajectoryDetail.tsx   # Detail panel (Summary/Payload/Result/Timing/Usage)
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

### Config Hub

A second top-level view (top-bar switch, next to Settings) that unifies each
AI CLI's configuration. The principle: **what can be shared lives once in
`~/.agents/` and is linked out; what can't falls back to per-tool handling.**

| Resource | Strategy | Delivery |
|----------|----------|----------|
| Skills | Linked | `~/.agents/skills/<name>` → `<tool>/skills/<name>` |
| Agents | Linked (Claude only) | `~/.agents/agents/<name>.md` → `<tool>/agents/<name>.md` |
| Prompts | Read-only (this stage) | `CLAUDE.md` / `AGENTS.md` |
| MCP / configs | Read-only (this stage) | JSON / TOML / YAML, shown per tool |

Per-tool state (`AppState`): `Linked` (symlink into the SSOT), `Copied` (a
copy this app made, tracked in `~/.trajectory-viewer/config-hub.json`),
`Drifted` (present but unmanaged — a real copy, or a link pointing elsewhere),
`Absent`.

`SyncMethod::Auto` (the default) prefers a symlink and silently falls back to
a directory copy when linking fails — Windows without developer mode, or a
cross-volume target. No junctions. Deletes reuse `trash.rs`, so a resource is
always recoverable.

Adding a tool: implement `ToolProvider` in `config_hub/<name>.rs`, register it
in `registry::providers()`, and add a `mod` line in `config_hub/mod.rs`.

### Adding a New Provider

A provider is a `SessionProvider` impl; the registry drives everything else
(scanning, messages, trash, and the UI's tool list).

1. Create `src-tauri/src/session_manager/{name}.rs` implementing
   `SessionProvider` (see `claude.rs` for a file-backed example, `opencode.rs`
   for one that delegates). Key methods: `id`, `sessions_dir`, `parse_session`,
   `load_messages`, `trash_session`, `trash_sessions_in_dir`.
   - Override `owns_source` if sessions aren't referenced by path (e.g.
     OpenCode's `sqlite:<db>:<id>`).
   - Override `file_extensions`/`scan` if storage isn't a plain file tree.
2. Register it in `provider::providers()` (`session_manager/provider.rs`).
3. Add a `mod {name};` line in `session_manager/mod.rs`.

For the trajectory (timeline/table) view, also add a parser:

4. Create `src-tauri/src/trajectory/parser/{name}.rs` with
   `parse_trajectory(path) -> Result<(String, Vec<TrajectoryEvent>), String>`
5. Add `pub mod {name};` to `trajectory/parser/mod.rs`
6. Add detection to `detect_provider()` and a route in `parse_trajectory()`
   (`trajectory/mod.rs`)

Finally, add the provider to the frontend lists: `PROVIDER_OPTIONS` in
`src/components/SettingsView.tsx`, `PROVIDER_CHIPS` in
`src/components/SessionBrowser.tsx`, and `AVAILABLE_PROVIDERS` in `src/App.tsx`.

### Testing

Rust: `cargo test` (unit + integration; `cargo test -- --ignored` runs the
real-data smoke tests). CI also enforces `cargo fmt --check` and
`cargo clippy --all-targets -- -D warnings`.

Frontend: `pnpm test` (Vitest, pure logic in `src/utils/`), `npx tsc --noEmit`.
