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
│   ├── App.tsx                   # Root: browser ↔ settings ↔ standalone file view
│   ├── api.ts                    # Tauri invoke wrapper
│   ├── types.ts                  # Shared interfaces
│   ├── styles.css                # Tailwind base + CSS variables (light/dark)
│   ├── lib/utils.ts              # cn() classname merge utility
│   ├── utils/
│   │   ├── layout.ts             # deriveTrajectoryLayout() — event→turn-grouped layout
│   │   ├── format.ts             # Time/token formatting utilities
│   │   └── {layout,format}.test.ts  # Vitest unit tests
│   └── components/
│       ├── SessionBrowser.tsx     # Main view: sidebar + messages/trajectory tabs
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
