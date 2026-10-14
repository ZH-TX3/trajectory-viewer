// ── Markdown editing ─────────────────────────────────────────────────────
//
// The edit/save/revert state machine shared by every Markdown editor in the
// app: the Config Hub's config files, and a project's memory documents.
//
// It owns nothing about where the file lives — `save` is passed in, so the
// caller decides the backend call (`config_hub_save_config` with its path
// allowlist, or `save_project_memory`, which resolves the document inside the
// project's memory directory).
//
// The draft starts as the file's text, not empty, so Save is only ever enabled
// by a real edit and the textarea can never be saved over a file with a blank
// body.

import { useCallback, useState } from 'react';

export interface MarkdownEditor {
  /** The text under edit. Empty while not editing. */
  draft: string;
  editing: boolean;
  saving: boolean;
  /** The last failure, or `null`. Surfaced to the user, never swallowed. */
  error: string | null;
  /** True when the draft differs from what was loaded. */
  dirty: boolean;
  startEdit: () => void;
  cancel: () => void;
  /** Undo edits back to the loaded text, staying in the editor. */
  revert: () => void;
  setDraft: (value: string) => void;
  /** Report a failure that is not the save itself (e.g. bad JSON to format). */
  setError: (message: string | null) => void;
  save: () => Promise<void>;
}

export interface UseMarkdownEditorOptions {
  /** The file's current content. */
  original: string;
  /** Writes `content` somewhere that can fail. */
  save: (content: string) => Promise<void>;
  /** Called after a successful save, with the body that was written. */
  onSaved?: (content: string) => void;
}

export function useMarkdownEditor({
  original,
  save,
  onSaved,
}: UseMarkdownEditorOptions): MarkdownEditor {
  const [draft, setDraft] = useState('');
  const [editing, setEditing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const startEdit = useCallback(() => {
    setDraft(original);
    setError(null);
    setEditing(true);
  }, [original]);

  const cancel = useCallback(() => {
    setEditing(false);
    setDraft('');
    setError(null);
  }, []);

  const revert = useCallback(() => {
    setDraft(original);
    setError(null);
  }, [original]);

  const handleSave = useCallback(async () => {
    setSaving(true);
    setError(null);
    try {
      await save(draft);
      setEditing(false);
      setDraft('');
      onSaved?.(draft);
    } catch (err) {
      // A failed write must leave the draft intact — losing the edit along
      // with the error is how work gets destroyed.
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }, [draft, save, onSaved]);

  return {
    draft,
    editing,
    saving,
    error,
    dirty: draft !== original,
    startEdit,
    cancel,
    revert,
    setDraft,
    setError,
    save: handleSave,
  };
}
