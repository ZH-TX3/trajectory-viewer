// ── Resume Launcher Settings ─────────────────────────────────────────────
//
// One command per provider: each tool ships its own CLI, so a single shared
// command cannot work (a Codex session resumed with `claude --resume` is
// nonsense).
//
// A shell alias or function (e.g. `cc`, often `claude --dangerously-skip-
// permissions`) works too: it is not a program, so the launcher runs it
// through an interactive shell with the profile loaded instead of exec'ing it.

import { useEffect, useRef, useState } from 'react';
import { Loader2, Check, AlertCircle, Terminal, MonitorPlay } from 'lucide-react';
import { api } from '../api';
import { cn } from '../lib/utils';
import type { ProviderCommand, ResumeSettings, TerminalStatus } from '../types';

const PROVIDER_LABELS: Record<string, string> = {
  claude: 'Claude Code',
  codex: 'Codex',
  dsh: 'DSH',
  opencode: 'OpenCode',
};

/**
 * Commands offered in the dropdown, per provider.
 *
 * The first entry is the provider's own CLI; the rest are the short wrappers
 * people commonly define (`cc` = `claude --dangerously-skip-permissions`). A
 * name that isn't installed is still selectable — the badge next to it says
 * `not found`, so a wrapper can be configured before it exists.
 */
const COMMAND_CHOICES: Record<string, string[]> = {
  claude: ['claude', 'cc', 'c'],
  codex: ['codex'],
  dsh: ['dsh-tui', 'dsh'],
  opencode: ['opencode'],
};

/** One provider's command field, with its resolution badge. */
function CommandRow({
  providerId,
  status,
  onChange,
  checking,
}: {
  providerId: string;
  status: ProviderCommand | undefined;
  onChange: (value: string) => void;
  checking: boolean;
}) {
  const [value, setValue] = useState(status?.command ?? '');

  // Follow the parent when it reloads (e.g. after another provider's save).
  useEffect(() => {
    if (status !== undefined) setValue(status.command);
  }, [status?.command]);

  const kind = status?.kind ?? null;
  const choices = COMMAND_CHOICES[providerId] ?? [providerId];
  // Keep a value that isn't in the list (a hand-set command) selectable, so
  // opening the settings never silently rewrites it.
  const options = choices.includes(value) || value === '' ? choices : [value, ...choices];

  return (
    <div className="flex items-center gap-2">
      <span className="w-20 shrink-0 text-[11px] text-muted-foreground">
        {PROVIDER_LABELS[providerId] ?? providerId}
      </span>
      <select
        value={value}
        onChange={(event) => {
          setValue(event.target.value);
          onChange(event.target.value);
        }}
        aria-label={`Resume command for ${providerId}`}
        className={cn(
          'w-40 px-2 py-1 rounded border bg-background text-xs font-mono',
          kind === 'missing' ? 'border-red-400/60 dark:border-red-700/60' : 'border-border/60',
        )}
      >
        {options.map((option) => (
          <option key={option} value={option}>
            {option}
          </option>
        ))}
      </select>
      {checking ? (
        <Loader2 className="size-3 animate-spin text-muted-foreground" />
      ) : kind === 'program' ? (
        <span className="inline-flex items-center gap-1 text-[10px] text-emerald-600 dark:text-emerald-400">
          <Check className="size-3" /> on PATH
        </span>
      ) : kind === 'alias' ? (
        <span
          className="inline-flex items-center gap-1 text-[10px] text-cyan-600 dark:text-cyan-400"
          title="Found in your shell profile; the tab runs it through an interactive shell"
        >
          <Terminal className="size-3" /> shell alias
        </span>
      ) : kind === 'missing' ? (
        <span className="inline-flex items-center gap-1 text-[10px] text-red-600 dark:text-red-400">
          <AlertCircle className="size-3" /> not found
        </span>
      ) : null}
    </div>
  );
}

/** Terminal picker: which app the resume tab opens in. */
function TerminalRow({
  terminals,
  value,
  onChange,
}: {
  terminals: TerminalStatus[];
  value: string;
  onChange: (value: string) => void;
}) {
  const selected = terminals.find((t) => t.id === value);
  // A pick that isn't installed (or is gone after an uninstall) still has to be
  // selectable, so opening the settings never silently rewrites it.
  const options =
    selected !== undefined || value === '' ? terminals : [{ ...terminals[0], id: value, label: value }, ...terminals];

  return (
    <div className="flex items-center gap-2 pt-3 mt-1 border-t border-border/40">
      <span className="w-20 shrink-0 text-[11px] text-muted-foreground">Terminal</span>
      <select
        value={value}
        onChange={(event) => onChange(event.target.value)}
        aria-label="Terminal used by the resume button"
        className={cn(
          'w-40 px-2 py-1 rounded border bg-background text-xs',
          selected !== undefined && !selected.available
            ? 'border-red-400/60 dark:border-red-700/60'
            : 'border-border/60',
        )}
      >
        {options.map((option) => (
          <option key={option.id} value={option.id}>
            {option.label}
            {option.available ? '' : ' — not installed'}
          </option>
        ))}
      </select>
      {selected === undefined || selected.isDefault ? (
        <span
          className="inline-flex items-center gap-1 text-[10px] text-muted-foreground"
          title="Picks the best terminal installed on this machine"
        >
          <MonitorPlay className="size-3" /> auto
        </span>
      ) : selected.available ? (
        <span className="inline-flex items-center gap-1 text-[10px] text-emerald-600 dark:text-emerald-400">
          <Check className="size-3" /> installed
        </span>
      ) : (
        <span className="inline-flex items-center gap-1 text-[10px] text-red-600 dark:text-red-400">
          <AlertCircle className="size-3" /> not installed
        </span>
      )}
    </div>
  );
}

/**
 * In-flight/completed probe results, shared across mounts.
 *
 * The settings pane is unmounted whenever the user switches to another view, so
 * without this every visit re-ran the whole probe — including a shell spawn to
 * resolve an alias. The backend memoises that too, but caching here also saves
 * the round trip and keeps the pane from flashing its "checking" state.
 */
let probeCache: Promise<[ProviderCommand[], TerminalStatus[], ResumeSettings]> | null = null;

/** Drop the cached probe, so the next read reflects a just-saved change. */
function invalidateProbeCache() {
  probeCache = null;
}

export function ResumeSection() {
  const [statuses, setStatuses] = useState<ProviderCommand[]>([]);
  const [terminals, setTerminals] = useState<TerminalStatus[]>([]);
  const [terminal, setTerminal] = useState('auto');
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The latest edit per provider, and the debounce timer that saves it.
  const draft = useRef<Record<string, string>>({});
  // Mirrors `terminal` for the debounced save, which would otherwise close over
  // a stale value.
  const terminalRef = useRef('auto');
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        probeCache ??= Promise.all([
          api.resumeCommandStatuses(),
          api.resumeTerminals(),
          api.getResumeSettings(),
        ]);
        const [commandStatuses, terminalList, settings] = await probeCache;
        if (cancelled) return;
        setStatuses(commandStatuses);
        setTerminals(terminalList);
        const saved = settings.terminal?.trim();
        const effective = saved !== undefined && saved !== '' ? saved : 'auto';
        setTerminal(effective);
        terminalRef.current = effective;
        // Seed the draft so a later command edit keeps the stored terminal.
        draft.current = { ...settings.commands };
      } catch (err) {
        // A failed probe must not be cached, or the pane could never recover.
        invalidateProbeCache();
        if (!cancelled) setError(String(err));
      }
    })();
    return () => {
      cancelled = true;
      if (timer.current !== null) clearTimeout(timer.current);
    };
  }, []);

  // Save + re-probe on edit (debounced). Saving first is what makes the probe
  // meaningful: the status command inspects the STORED value.
  const handleChange = (value: string, providerId?: string, nextTerminal?: string) => {
    if (providerId !== undefined) {
      draft.current = { ...draft.current, [providerId]: value };
    }
    // Read from the ref, not the state: the debounced closure would otherwise
    // capture the terminal as it was before this change.
    const terminalToSave = nextTerminal ?? terminalRef.current;
    if (nextTerminal !== undefined) terminalRef.current = nextTerminal;
    setChecking(true);
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      void (async () => {
        try {
          await api.setResumeSettings({
            commands: draft.current,
            terminal: terminalToSave,
          });
          // The stored value changed, so the cached probe is stale.
          invalidateProbeCache();
          setStatuses(await api.resumeCommandStatuses());
          setError(null);
        } catch (err) {
          setError(String(err));
        } finally {
          setChecking(false);
        }
      })();
    }, 600);
  };

  const handleTerminalChange = (value: string) => {
    setTerminal(value);
    handleChange(value, undefined, value);
  };

  return (
    <div className="px-4 py-3 space-y-2">
      {statuses.map((status) => (
        <CommandRow
          key={status.providerId}
          providerId={status.providerId}
          status={status}
          checking={checking}
          onChange={(value) => handleChange(value, status.providerId)}
        />
      ))}

      {terminals.length > 0 && (
        <TerminalRow terminals={terminals} value={terminal} onChange={handleTerminalChange} />
      )}

      <p className="text-[10px] text-muted-foreground pt-1">
        The tab runs{' '}
        <span className="font-mono">&lt;command&gt; --resume &lt;session-id&gt;</span> in the
        session's own directory. Aliases work — a name like{' '}
        <span className="font-mono">cc</span> is looked up in your shell profile, so its own flags
        apply. Leave a command empty to use that tool's default; if the chosen terminal isn't
        installed, the system default is used instead.
      </p>
      {error !== null && (
        <p className="text-[10px] text-red-600 dark:text-red-400">{error}</p>
      )}
    </div>
  );
}
