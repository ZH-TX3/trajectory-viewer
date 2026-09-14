// ── Settings View ────────────────────────────────────────────────────────
//
// Full-page settings (pattern: cc-switch) split into two tabs:
//   General  → which tools are scanned + their display order (drag to reorder)
//   Advanced → session backup & restore (cc-switch setup)
// Future sections slot in as more tabs or sections below.

import { useState } from 'react';
import type { CSSProperties } from 'react';
import {
  DndContext,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from '@dnd-kit/core';
import {
  SortableContext,
  arrayMove,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import { ArrowLeft, Settings, GripVertical } from 'lucide-react';
import { cn } from '../lib/utils';
import { BackupSection } from './BackupSection';
import { TrashSection } from './TrashSection';

type SettingsTab = 'general' | 'advanced';

interface SettingsViewProps {
  enabledProviders: ReadonlySet<string>;
  providerOrder: readonly string[];
  onToggleProvider: (id: string) => void;
  onReorderOrder: (order: string[]) => void;
  onBack: () => void;
}

interface ProviderOption {
  id: string;
  label: string;
  description: string;
  supported: boolean;
}

const PROVIDER_OPTIONS: ProviderOption[] = [
  { id: 'claude', label: 'Claude Code', description: 'Scans ~/.claude/projects', supported: true },
  { id: 'codex', label: 'Codex', description: 'Scans ~/.codex/sessions', supported: true },
  { id: 'dsh', label: 'DSH', description: 'Scans ~/.dsh/sessions', supported: true },
  { id: 'opencode', label: 'OpenCode', description: 'Scans ~/.local/share/opencode', supported: true },
];

/** One sortable row: the drag handle owns attributes/listeners, the rest of
 * the row stays clickable for its checkbox. */
function SortableRow({
  opt,
  enabled,
  onToggle,
}: {
  opt: ProviderOption;
  enabled: boolean;
  onToggle: (id: string) => void;
}) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: opt.id,
  });
  const style: CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
  };

  return (
    <div
      ref={setNodeRef}
      style={style}
      className={cn(
        'flex items-center gap-3 px-4 py-3 border-b border-border/40 last:border-b-0',
        opt.supported ? 'hover:bg-muted/30' : 'opacity-50',
        isDragging && 'opacity-40 shadow-md z-10 relative',
      )}
    >
      <button
        type="button"
        {...attributes}
        {...listeners}
        disabled={!opt.supported}
        aria-label={`Reorder ${opt.label}`}
        title="Drag to reorder"
        className="shrink-0 p-1 -m-1 rounded hover:bg-muted/60 text-muted-foreground/40 hover:text-muted-foreground cursor-grab active:cursor-grabbing touch-none"
      >
        <GripVertical className="size-3.5" />
      </button>
      <input
        type="checkbox"
        checked={enabled}
        disabled={!opt.supported}
        onChange={() => onToggle(opt.id)}
        className="accent-blue-500"
      />
      <div className="flex-1 min-w-0">
        <div className="text-sm flex items-center gap-2">
          {opt.label}
          {!opt.supported && (
            <span className="text-[9px] px-1 py-0.5 rounded bg-muted/60 text-muted-foreground">not yet</span>
          )}
        </div>
        <div className="text-[11px] text-muted-foreground truncate">{opt.description}</div>
      </div>
    </div>
  );
}

export function SettingsView({
  enabledProviders,
  providerOrder,
  onToggleProvider,
  onReorderOrder,
  onBack,
}: SettingsViewProps) {
  const [tab, setTab] = useState<SettingsTab>('general');
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  // Render options in the user-defined display order.
  const byId = new Map(PROVIDER_OPTIONS.map((opt) => [opt.id, opt]));
  const orderedOptions = providerOrder
    .map((id) => byId.get(id))
    .filter((opt): opt is ProviderOption => opt !== undefined);
  const orderedIds = orderedOptions.map((opt) => opt.id);

  const handleDragEnd = (event: DragEndEvent) => {
    const { active, over } = event;
    if (over === null || active.id === over.id) return;
    const oldIndex = orderedIds.indexOf(String(active.id));
    const newIndex = orderedIds.indexOf(String(over.id));
    if (oldIndex < 0 || newIndex < 0) return;
    onReorderOrder(arrayMove(orderedIds, oldIndex, newIndex));
  };

  const TABS: Array<{ id: SettingsTab; label: string }> = [
    { id: 'general', label: 'General' },
    { id: 'advanced', label: 'Advanced' },
  ];

  return (
    <div className="h-full flex flex-col bg-white dark:bg-gray-900">
      <header className="h-10 border-b border-border/40 flex items-center gap-2 px-3 shrink-0 bg-white dark:bg-gray-900">
        <button
          onClick={onBack}
          title="Back"
          aria-label="Back"
          className="flex items-center justify-center size-7 shrink-0 rounded-md border border-border/60 text-muted-foreground hover:text-foreground hover:bg-muted/40 transition-colors"
        >
          <ArrowLeft className="size-4" />
        </button>
        <h1 className="ml-2 text-base font-semibold leading-none">Settings</h1>
      </header>

      {/* Tab row — below the header, full width */}
      <div className="flex items-center gap-1 px-3 py-1.5 border-b border-border/40 shrink-0 bg-white dark:bg-gray-900">
        {TABS.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={cn(
              'px-2.5 py-1 rounded text-[11px] font-medium transition-colors',
              tab === t.id
                ? 'bg-blue-100 dark:bg-blue-900/50 text-blue-700 dark:text-blue-200 shadow-sm'
                : 'text-muted-foreground hover:text-foreground hover:bg-muted/40',
            )}
          >
            {t.label}
          </button>
        ))}
      </div>

      <div className="flex-1 overflow-auto">
        <div className="max-w-xl mx-auto w-full p-4 space-y-4">
          {tab === 'general' && (
            <section className="rounded-xl border border-border/40 overflow-hidden">
              <div className="px-4 py-3 border-b border-border/40">
                <h2 className="text-xs font-medium">Session tools</h2>
                <p className="text-[10px] text-muted-foreground mt-0.5">
                  Choose which tools are scanned into the session list. Drag by the handle to reorder.
                </p>
              </div>
              <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
                <SortableContext items={orderedIds} strategy={verticalListSortingStrategy}>
                  {orderedOptions.map((opt) => (
                    <SortableRow
                      key={opt.id}
                      opt={opt}
                      enabled={enabledProviders.has(opt.id)}
                      onToggle={onToggleProvider}
                    />
                  ))}
                </SortableContext>
              </DndContext>
            </section>
          )}

          {tab === 'advanced' && (
            <>
              <section className="rounded-xl border border-border/40 overflow-hidden">
                <div className="px-4 py-3 border-b border-border/40">
                  <h2 className="text-xs font-medium">Session backup & restore</h2>
                  <p className="text-[10px] text-muted-foreground mt-0.5">
                    Pack the AI tools' session directories into zips in{' '}
                    <span className="font-mono">~/.trajectory-viewer/backups/</span>, auto-backup on an
                    interval, or restore from one (with a safety copy first).
                  </p>
                </div>
                <BackupSection />
              </section>

              <section className="rounded-xl border border-border/40 overflow-hidden">
                <div className="px-4 py-3 border-b border-border/40">
                  <h2 className="text-xs font-medium">Trash</h2>
                  <p className="text-[10px] text-muted-foreground mt-0.5">
                    Deleted sessions land here and can be restored. Entries older than 30 days are
                    auto-purged.
                  </p>
                </div>
                <TrashSection />
              </section>
            </>
          )}
        </div>
      </div>
    </div>
  );
}