// ── Settings View ────────────────────────────────────────────────────────
//
// Full-page settings (pattern: cc-switch). Hosts the "tools to load" toggles
// today; future settings slots in as more sections.

import { ArrowLeft, Settings } from 'lucide-react';
import { cn } from '../lib/utils';

interface SettingsViewProps {
  enabledProviders: ReadonlySet<string>;
  onToggleProvider: (id: string) => void;
  onBack: () => void;
}

const PROVIDER_OPTIONS: Array<{ id: string; label: string; description: string; supported: boolean }> = [
  { id: 'claude', label: 'Claude Code', description: 'Scans ~/.claude/projects', supported: true },
  { id: 'codex', label: 'Codex', description: 'Scans ~/.codex/sessions', supported: true },
  { id: 'dsh', label: 'DSH', description: 'Scans ~/.dsh/sessions', supported: true },
  { id: 'opencode', label: 'OpenCode', description: 'Scans ~/.local/share/opencode', supported: true },
];

export function SettingsView({ enabledProviders, onToggleProvider, onBack }: SettingsViewProps) {
  return (
    <div className="h-full flex flex-col bg-white dark:bg-gray-900">
      <header className="h-10 border-b border-border/40 flex items-center gap-2 px-3 shrink-0 bg-white dark:bg-gray-900">
        <button
          onClick={onBack}
          className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground transition-colors"
        >
          <ArrowLeft className="size-3.5" />
          Back
        </button>
        <Settings className="size-4 text-muted-foreground" />
        <span className="text-sm">Settings</span>
      </header>

      <div className="flex-1 overflow-auto">
        <div className="max-w-xl mx-auto w-full p-4 space-y-4">
          <section className="rounded-xl border border-border/40 overflow-hidden">
            <div className="px-4 py-3 border-b border-border/40">
              <h2 className="text-xs font-medium">Session tools</h2>
              <p className="text-[10px] text-muted-foreground mt-0.5">
                Choose which tools are scanned into the session list.
              </p>
            </div>
            {PROVIDER_OPTIONS.map((opt) => (
              <label
                key={opt.id}
                className={cn(
                  'flex items-center gap-3 px-4 py-3 cursor-pointer select-none border-b border-border/40 last:border-b-0',
                  opt.supported ? 'hover:bg-muted/30' : 'opacity-50 cursor-not-allowed',
                )}
              >
                <input
                  type="checkbox"
                  checked={enabledProviders.has(opt.id)}
                  disabled={!opt.supported}
                  onChange={() => onToggleProvider(opt.id)}
                  className="accent-blue-500"
                />
                <div className="flex-1 min-w-0">
                  <div className="text-sm flex items-center gap-2">
                    {opt.label}
                    {!opt.supported && (
                      <span className="text-[9px] px-1 py-0.5 rounded bg-muted/60 text-muted-foreground">
                        not yet
                      </span>
                    )}
                  </div>
                  <div className="text-[11px] text-muted-foreground truncate">{opt.description}</div>
                </div>
              </label>
            ))}
          </section>
        </div>
      </div>
    </div>
  );
}