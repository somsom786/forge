// Pure helpers for Harness Studio.
import type { HarnessItem, HarnessKind, HarnessPreview, HookDryRun, HookSpec } from './types';

export const KIND_ORDER: HarnessKind[] = ['instructions', 'settings', 'rule', 'skill', 'subagent', 'command', 'mcp'];
export const KIND_LABEL: Record<HarnessKind, string> = {
  instructions: 'Instructions', settings: 'Settings', rule: 'Rules', skill: 'Skills', subagent: 'Subagents', command: 'Commands', mcp: 'MCP',
};

export function groupInventory(items: HarnessItem[]): { kind: HarnessKind; label: string; items: HarnessItem[] }[] {
  return KIND_ORDER.map((kind) => ({
    kind,
    label: KIND_LABEL[kind],
    items: items.filter((i) => i.kind === kind).sort((a, b) => a.name.localeCompare(b.name)),
  })).filter((g) => g.items.length > 0);
}

/** Claude Code hook events a hook may be registered for. */
export const HOOK_EVENTS = [
  'PreToolUse', 'PostToolUse', 'UserPromptSubmit', 'Notification', 'Stop', 'SubagentStop', 'PreCompact', 'SessionStart', 'SessionEnd',
] as const;

// ---- diff classification ----------------------------------------------------------------------------

export type DiffLineKind = 'add' | 'del' | 'ctx' | 'hunk' | 'meta';
export interface ClassifiedLine { kind: DiffLineKind; text: string }

/** Classifies unified diff lines for colouring. Never produces markup; callers render text nodes. */
export function classifyDiff(diff: string): ClassifiedLine[] {
  if (!diff) return [];
  const lines = diff.split(/\r?\n/);
  if (lines[lines.length - 1] === '') lines.pop();
  let inHunk = false;
  return lines.map((text): ClassifiedLine => {
    if (text.startsWith('diff ')) { inHunk = false; return { kind: 'meta', text }; }
    if (text.startsWith('@@')) { inHunk = true; return { kind: 'hunk', text }; }
    if (!inHunk || text.startsWith('\\')) return { kind: 'meta', text };
    if (text.startsWith('+')) return { kind: 'add', text };
    if (text.startsWith('-')) return { kind: 'del', text };
    return { kind: 'ctx', text };
  });
}

// ---- new file paths ---------------------------------------------------------------------------------

export type NewFileType = 'CLAUDE.md' | 'AGENTS.md' | 'GEMINI.md' | 'settings' | 'rule' | 'command' | 'agent' | 'skill';

export const NEW_FILE_TYPES: { type: NewFileType; label: string; needsName: boolean }[] = [
  { type: 'CLAUDE.md', label: 'CLAUDE.md (project instructions)', needsName: false },
  { type: 'AGENTS.md', label: 'AGENTS.md', needsName: false },
  { type: 'GEMINI.md', label: 'GEMINI.md', needsName: false },
  { type: 'settings', label: '.claude/settings.json', needsName: false },
  { type: 'rule', label: 'Rule (.claude/rules/<name>.md)', needsName: true },
  { type: 'command', label: 'Slash command (.claude/commands/<name>.md)', needsName: true },
  { type: 'agent', label: 'Subagent (.claude/agents/<name>.md)', needsName: true },
  { type: 'skill', label: 'Skill (.claude/skills/<name>/SKILL.md)', needsName: true },
];

const NAME_RE = /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/;

export function validateName(name: string): string | null {
  const n = name.trim();
  if (!n) return 'Enter a name.';
  if (n.includes('..')) return 'Names cannot contain "..".';
  if (!NAME_RE.test(n)) return 'Use letters, digits, ".", "_" or "-" (max 64, starting with a letter or digit).';
  return null;
}

export function newFilePath(type: NewFileType, name: string): { ok: true; rel: string } | { ok: false; error: string } {
  const spec = NEW_FILE_TYPES.find((t) => t.type === type);
  if (!spec) return { ok: false, error: 'Unknown file type.' };
  if (!spec.needsName) return { ok: true, rel: type === 'settings' ? '.claude/settings.json' : type };
  const bare = name.trim().replace(/\.md$/i, '');
  const err = validateName(bare);
  if (err) return { ok: false, error: err };
  switch (type) {
    case 'rule': return { ok: true, rel: `.claude/rules/${bare}.md` };
    case 'command': return { ok: true, rel: `.claude/commands/${bare}.md` };
    case 'agent': return { ok: true, rel: `.claude/agents/${bare}.md` };
    default: return { ok: true, rel: `.claude/skills/${bare}/SKILL.md` };
  }
}

export function starterText(type: NewFileType, name: string): string {
  const n = name.trim().replace(/\.md$/i, '') || 'name';
  switch (type) {
    case 'CLAUDE.md': return '# Project instructions\n\nDescribe how to build, test and work in this repository.\n';
    case 'AGENTS.md': return '# Agent instructions\n\n';
    case 'GEMINI.md': return '# Instructions\n\n';
    case 'settings': return '{\n}\n';
    case 'rule': return `# ${n}\n\n`;
    case 'command': return `---\ndescription: ${n}\n---\n\n`;
    case 'agent': return `---\nname: ${n}\ndescription: When to use this subagent\n---\n\nYou are a specialised subagent.\n`;
    case 'skill': return `---\nname: ${n}\ndescription: When this skill should be used\n---\n\n# ${n}\n`;
  }
}

// ---- hooks ------------------------------------------------------------------------------------------

/** Characters that must never be hidden from the user: control, zero-width, bidi, and other non-ASCII. */
export function oddChars(text: string): { char: string; code: string; index: number }[] {
  const out: { char: string; code: string; index: number }[] = [];
  let i = 0;
  for (const ch of text) {
    const cp = ch.codePointAt(0)!;
    const odd = cp < 0x20 || cp === 0x7f || cp > 0x7e;
    if (odd) out.push({ char: ch, code: `U+${cp.toString(16).toUpperCase().padStart(4, '0')}`, index: i });
    i += ch.length;
  }
  return out;
}

/** Text with odd characters replaced by visible ⟦U+XXXX⟧ markers. */
export function revealOdd(text: string): string {
  let out = '';
  for (const ch of text) {
    const cp = ch.codePointAt(0)!;
    out += cp < 0x20 || cp === 0x7f || cp > 0x7e ? `⟦U+${cp.toString(16).toUpperCase().padStart(4, '0')}⟧` : ch;
  }
  return out;
}

export function validateHookSpec(spec: { event: string; matcher: string; command: string; timeoutSecs: number }): string[] {
  const p: string[] = [];
  if (!spec.event) p.push('Choose an event.');
  if (!spec.command.trim()) p.push('Enter a command.');
  if (/[\r\n]/.test(spec.command)) p.push('The command must be a single line.');
  if (/[\r\n]/.test(spec.matcher)) p.push('The matcher must be a single line.');
  if (!Number.isInteger(spec.timeoutSecs) || spec.timeoutSecs < 1 || spec.timeoutSecs > 600) p.push('Timeout must be a whole number from 1 to 600 seconds.');
  return p;
}

export function customHookSpec(spec: { event: string; matcher: string; command: string; timeoutSecs: number }): HookSpec {
  return { event: spec.event, matcher: spec.matcher.trim() || null, command: spec.command, timeoutSecs: spec.timeoutSecs };
}

// ---- saving -----------------------------------------------------------------------------------------

export function canSave(p: Pick<HarnessPreview, 'problems' | 'risks' | 'unchanged'>, acknowledged: boolean): boolean {
  if (p.problems.length > 0 || p.unchanged) return false;
  return p.risks.length === 0 || acknowledged;
}

export function isChangedOnDisk(message: string): boolean {
  return /changed on disk/i.test(message);
}

// ---- dry run view model -----------------------------------------------------------------------------

export interface DryRunView {
  outcome: 'timed_out' | 'exit_0' | 'exit_nonzero' | 'no_exit';
  headline: string;
  tone: 'ok' | 'bad' | 'warn';
  rows: [string, string][];
  withheldEnv: string[];
  inputJson: string;
}

export function dryRunView(r: HookDryRun): DryRunView {
  const outcome = r.timedOut ? 'timed_out' : r.exitCode === null ? 'no_exit' : r.exitCode === 0 ? 'exit_0' : 'exit_nonzero';
  const headline =
    outcome === 'timed_out' ? 'Timed out' : outcome === 'no_exit' ? 'No exit code' : outcome === 'exit_0' ? 'Exit code 0' : `Exit code ${r.exitCode}`;
  let inputJson: string;
  try { inputJson = JSON.stringify(r.input, null, 2) ?? ''; } catch { inputJson = String(r.input); }
  return {
    outcome,
    headline,
    tone: outcome === 'exit_0' ? 'ok' : outcome === 'exit_nonzero' ? 'bad' : 'warn',
    rows: [
      ['Shell', r.shell],
      ['Exit code', r.exitCode === null ? '—' : String(r.exitCode)],
      ['Timed out', r.timedOut ? 'yes' : 'no'],
      ['Duration', `${r.durationMs} ms`],
    ],
    withheldEnv: r.scrubbedEnv,
    inputJson,
  };
}

// ---- invisible / direction-changing characters ------------------------------------------------------

/** U+200B-200F, U+202A-202E, U+2060-2069, U+FEFF, and control characters except newline and tab. */
export function isInvisibleChar(cp: number): boolean {
  return (
    (cp >= 0x200b && cp <= 0x200f) || (cp >= 0x202a && cp <= 0x202e) || (cp >= 0x2060 && cp <= 0x2069) || cp === 0xfeff ||
    ((cp < 0x20 || cp === 0x7f) && cp !== 0x0a && cp !== 0x09)
  );
}

export interface TextSegment { text: string; invisible: boolean }

/** Splits text so invisible characters become their own `<U+XXXX>` segments (rendered in a distinct style). */
export function invisibleSegments(text: string): TextSegment[] {
  const out: TextSegment[] = [];
  let buf = '';
  for (const ch of text) {
    const cp = ch.codePointAt(0)!;
    if (isInvisibleChar(cp)) {
      if (buf) { out.push({ text: buf, invisible: false }); buf = ''; }
      out.push({ text: `<U+${cp.toString(16).toUpperCase().padStart(4, '0')}>`, invisible: true });
    } else buf += ch;
  }
  if (buf) out.push({ text: buf, invisible: false });
  return out;
}

export function escapeInvisible(text: string): string {
  return invisibleSegments(text).map((s) => s.text).join('');
}

export function hasInvisible(text: string): boolean {
  return invisibleSegments(text).some((s) => s.invisible);
}
