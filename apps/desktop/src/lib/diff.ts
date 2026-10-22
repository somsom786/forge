// Unified diff parser (git format). Pure; rendering lives in components/DiffViewer.tsx.

export type DiffLineType = 'add' | 'del' | 'ctx';

export interface DiffLine {
  type: DiffLineType;
  oldNo: number | null;
  newNo: number | null;
  text: string;
  /** Followed by "\ No newline at end of file". */
  noNewline?: boolean;
}

export interface DiffHunk {
  header: string;
  section: string;
  oldStart: number;
  oldCount: number;
  newStart: number;
  newCount: number;
  lines: DiffLine[];
}

export type FileStatus = 'modified' | 'added' | 'deleted' | 'renamed' | 'copied';

export interface DiffFile {
  oldPath: string | null;
  newPath: string | null;
  /** Path to show: new path, or old path for deletions. */
  path: string;
  status: FileStatus;
  binary: boolean;
  similarity: number | null;
  modeChange: string | null;
  hunks: DiffHunk[];
  added: number;
  removed: number;
  /** Verbatim extended header lines worth surfacing (mode changes, etc). */
  notes: string[];
}

const HUNK_RE = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@ ?(.*)$/;

function unquote(p: string): string {
  if (p.length >= 2 && p.startsWith('"') && p.endsWith('"')) {
    return p
      .slice(1, -1)
      .replace(/\\(["\\])/g, '$1')
      .replace(/\\t/g, '\t')
      .replace(/\\n/g, '\n');
  }
  return p;
}

function stripPrefix(p: string): string | null {
  const u = unquote(p.trim());
  if (u === '/dev/null') return null;
  return u.replace(/^[ab]\//, '');
}

/** Best-effort split of "a/x b/y" (ambiguous with spaces; refined later by ---/+++ and rename lines). */
function splitGitHeader(rest: string): [string, string] {
  const half = rest.length >> 1;
  // Identical paths (the common case) split exactly in the middle.
  const left = rest.slice(0, half);
  const right = rest.slice(half + 1);
  if (rest[half] === ' ' && left.startsWith('a/') && right.startsWith('b/') && left.slice(2) === right.slice(2)) {
    return [left.slice(2), right.slice(2)];
  }
  const m = /^(".*?"|\S+) (".*"|\S+)$/.exec(rest);
  if (m) return [stripPrefix(m[1]) ?? m[1], stripPrefix(m[2]) ?? m[2]];
  const i = rest.indexOf(' b/');
  if (i > 0) return [rest.slice(0, i).replace(/^a\//, ''), rest.slice(i + 3)];
  return [rest, rest];
}

function newFile(oldPath: string | null, newPath: string | null): DiffFile {
  return {
    oldPath,
    newPath,
    path: newPath ?? oldPath ?? '(unknown)',
    status: 'modified',
    binary: false,
    similarity: null,
    modeChange: null,
    hunks: [],
    added: 0,
    removed: 0,
    notes: [],
  };
}

export function parseDiff(text: string): DiffFile[] {
  if (!text) return [];
  const lines = text.split(/\r?\n/);
  // A trailing newline yields one empty trailing element; it is not content.
  if (lines.length && lines[lines.length - 1] === '') lines.pop();

  const files: DiffFile[] = [];
  let file: DiffFile | null = null;
  let hunk: DiffHunk | null = null;
  let remOld = 0;
  let remNew = 0;
  let oldNo = 0;
  let newNo = 0;

  const finishPath = (f: DiffFile) => {
    f.path = f.newPath ?? f.oldPath ?? f.path;
  };

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    // Inside a hunk body: counts decide when it ends, so content like "--- x" or "diff --git" is safe.
    if (hunk && (remOld > 0 || remNew > 0)) {
      const c = line[0];
      if (c === '+' && remNew > 0) {
        hunk.lines.push({ type: 'add', oldNo: null, newNo: newNo++, text: line.slice(1) });
        remNew--;
        file!.added++;
        continue;
      }
      if (c === '-' && remOld > 0) {
        hunk.lines.push({ type: 'del', oldNo: oldNo++, newNo: null, text: line.slice(1) });
        remOld--;
        file!.removed++;
        continue;
      }
      if ((c === ' ' || line === '') && remOld > 0 && remNew > 0) {
        hunk.lines.push({ type: 'ctx', oldNo: oldNo++, newNo: newNo++, text: line.slice(1) });
        remOld--;
        remNew--;
        continue;
      }
      if (c === '\\') {
        const last = hunk.lines[hunk.lines.length - 1];
        if (last) last.noNewline = true;
        continue;
      }
      // Malformed counts: fall through and treat as a header line.
      remOld = 0;
      remNew = 0;
    }
    if (hunk && line.startsWith('\\')) {
      const last = hunk.lines[hunk.lines.length - 1];
      if (last) last.noNewline = true;
      continue;
    }

    if (line.startsWith('diff --git ')) {
      const [a, b] = splitGitHeader(line.slice('diff --git '.length));
      file = newFile(a, b);
      files.push(file);
      hunk = null;
      continue;
    }

    const hm = HUNK_RE.exec(line);
    if (hm && file) {
      hunk = {
        header: line,
        section: hm[5] ?? '',
        oldStart: Number(hm[1]),
        oldCount: hm[2] === undefined ? 1 : Number(hm[2]),
        newStart: Number(hm[3]),
        newCount: hm[4] === undefined ? 1 : Number(hm[4]),
        lines: [],
      };
      file.hunks.push(hunk);
      remOld = hunk.oldCount;
      remNew = hunk.newCount;
      oldNo = hunk.oldStart;
      newNo = hunk.newStart;
      continue;
    }

    // "--- " / "+++ " file headers (also starts a file for plain unified diffs without "diff --git").
    if (line.startsWith('--- ') && (!file || file.hunks.length > 0)) {
      const next = lines[i + 1];
      if (next !== undefined && next.startsWith('+++ ')) {
        file = newFile(stripPrefix(line.slice(4).replace(/\t.*$/, '')), stripPrefix(next.slice(4).replace(/\t.*$/, '')));
        if (file.oldPath === null) file.status = 'added';
        if (file.newPath === null) file.status = 'deleted';
        finishPath(file);
        files.push(file);
        hunk = null;
        i++;
        continue;
      }
    }
    if (!file) continue;

    if (line.startsWith('--- ')) {
      file.oldPath = stripPrefix(line.slice(4).replace(/\t.*$/, ''));
      finishPath(file);
    } else if (line.startsWith('+++ ')) {
      file.newPath = stripPrefix(line.slice(4).replace(/\t.*$/, ''));
      finishPath(file);
    } else if (line.startsWith('new file mode')) {
      file.status = 'added';
      file.oldPath = null;
      finishPath(file);
    } else if (line.startsWith('deleted file mode')) {
      file.status = 'deleted';
      file.newPath = null;
      finishPath(file);
    } else if (line.startsWith('rename from ')) {
      file.status = 'renamed';
      file.oldPath = unquote(line.slice('rename from '.length));
    } else if (line.startsWith('rename to ')) {
      file.status = 'renamed';
      file.newPath = unquote(line.slice('rename to '.length));
      finishPath(file);
    } else if (line.startsWith('copy from ')) {
      file.status = 'copied';
      file.oldPath = unquote(line.slice('copy from '.length));
    } else if (line.startsWith('copy to ')) {
      file.status = 'copied';
      file.newPath = unquote(line.slice('copy to '.length));
      finishPath(file);
    } else if (line.startsWith('similarity index ')) {
      file.similarity = parseInt(line.slice('similarity index '.length), 10);
    } else if (line.startsWith('old mode ') || line.startsWith('new mode ')) {
      file.modeChange = file.modeChange ? `${file.modeChange} -> ${line.slice(9)}` : `mode ${line.slice(9)}`;
      file.notes.push(line);
    } else if (line.startsWith('Binary files ') || line.startsWith('GIT binary patch')) {
      file.binary = true;
    } else if (line.startsWith('index ') || line.startsWith('dissimilarity index ')) {
      // not displayed
    } else if (line.trim() !== '') {
      file.notes.push(line);
    }
  }
  return files;
}

export interface SideRow {
  left: DiffLine | null;
  right: DiffLine | null;
}

/** Pair deletions with the additions that follow them for the side-by-side view. */
export function toSideBySide(hunk: DiffHunk): SideRow[] {
  const rows: SideRow[] = [];
  let i = 0;
  const ls = hunk.lines;
  while (i < ls.length) {
    const l = ls[i];
    if (l.type === 'ctx') {
      rows.push({ left: l, right: l });
      i++;
      continue;
    }
    const dels: DiffLine[] = [];
    const adds: DiffLine[] = [];
    while (i < ls.length && ls[i].type === 'del') dels.push(ls[i++]);
    while (i < ls.length && ls[i].type === 'add') adds.push(ls[i++]);
    const n = Math.max(dels.length, adds.length);
    for (let k = 0; k < n; k++) rows.push({ left: dels[k] ?? null, right: adds[k] ?? null });
  }
  return rows;
}
