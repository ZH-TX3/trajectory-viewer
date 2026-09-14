// ── Config Hub: read-only config panel ───────────────────────────────────
//
// Shows each tool's system prompt and config files. This stage is display
// only — nothing here writes — so the panel says so plainly and shows a tool's
// installed state rather than an empty box when the tool is missing.

import { useState } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
import type { ConfigFileContent, ToolConfigs } from '../../types';
import { cn } from '../../lib/utils';

interface ConfigReadonlyPanelProps {
  configs: ToolConfigs[];
}

export function ConfigReadonlyPanel({ configs }: ConfigReadonlyPanelProps) {
  return (
    <div className="space-y-2">
      <div className="text-[11px] text-muted-foreground">
        System prompts and config files. Display only — editing arrives in a later stage.
      </div>
      {configs.map((tool) => (
        <ToolConfigCard key={tool.toolId} tool={tool} />
      ))}
    </div>
  );
}

function ToolConfigCard({ tool }: { tool: ToolConfigs }) {
  const [open, setOpen] = useState(false);
  const files: ConfigFileContent[] = [
    ...(tool.prompt ? [tool.prompt] : []),
    ...tool.files,
  ];

  return (
    <div className="rounded-lg border border-border/40">
      <button
        onClick={() => setOpen((v) => !v)}
        disabled={!tool.installed}
        className="w-full flex items-center gap-2 px-3 py-2 text-left disabled:cursor-default"
      >
        {open ? (
          <ChevronDown className="size-3 text-muted-foreground" />
        ) : (
          <ChevronRight className="size-3 text-muted-foreground" />
        )}
        <span className="text-xs font-medium">{tool.displayName}</span>
        <span className="ml-auto text-[10px] text-muted-foreground">
          {tool.installed ? `${files.filter((f) => f.exists).length}/${files.length} files` : 'not installed'}
        </span>
      </button>

      {open && (
        <div className="border-t border-border/40 divide-y divide-border/40">
          {!tool.installed ? (
            <div className="px-3 py-2 text-[11px] text-muted-foreground">
              {tool.displayName} is not installed — nothing to show.
            </div>
          ) : (
            files.map((file) => <FileBlock key={file.path} file={file} />)
          )}
        </div>
      )}
    </div>
  );
}

function FileBlock({ file }: { file: ConfigFileContent }) {
  const [expanded, setExpanded] = useState(false);
  const content = file.content ?? '';

  return (
    <div className="px-3 py-2">
      <div className="flex items-center gap-2 mb-1">
        <span className="text-[11px] font-medium">{file.label}</span>
        <span className="text-[9px] uppercase tracking-wide text-muted-foreground/70 border border-border/40 rounded px-1">
          read-only
        </span>
        {!file.exists && <span className="text-[10px] text-muted-foreground">missing</span>}
        {file.exists && content.length > 400 && (
          <button
            onClick={() => setExpanded((v) => !v)}
            className="ml-auto text-[10px] text-blue-600 dark:text-blue-400 hover:underline"
          >
            {expanded ? 'collapse' : 'expand'}
          </button>
        )}
      </div>
      <div className="text-[10px] text-muted-foreground font-mono break-all mb-1">{file.path}</div>
      {file.exists ? (
        <pre
          className={cn(
            'text-[10px] leading-relaxed whitespace-pre-wrap break-all rounded bg-muted/30 p-2 overflow-auto',
            expanded ? 'max-h-96' : 'max-h-24',
          )}
        >
          {content || '(empty)'}
        </pre>
      ) : (
        <div className="text-[10px] text-muted-foreground">File does not exist.</div>
      )}
    </div>
  );
}
