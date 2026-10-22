import { describe, expect, it } from 'vitest';
import { parseDiff, toSideBySide } from './diff';

const MULTI = `diff --git a/src/a.ts b/src/a.ts
index 1111111..2222222 100644
--- a/src/a.ts
+++ b/src/a.ts
@@ -1,4 +1,4 @@ function one()
 line1
-line2
+line2 changed
 line3
 line4
@@ -20,3 +20,4 @@ function two()
 x
+inserted
 y
 z
`;

describe('parseDiff', () => {
  it('returns [] for empty input', () => {
    expect(parseDiff('')).toEqual([]);
  });

  it('parses multiple hunks with correct line numbers and counts', () => {
    const [f] = parseDiff(MULTI);
    expect(f.path).toBe('src/a.ts');
    expect(f.status).toBe('modified');
    expect(f.hunks).toHaveLength(2);
    expect(f.added).toBe(2);
    expect(f.removed).toBe(1);
    const h0 = f.hunks[0];
    expect(h0.section).toBe('function one()');
    expect(h0.lines.map((l) => l.type)).toEqual(['ctx', 'del', 'add', 'ctx', 'ctx']);
    expect(h0.lines[1]).toMatchObject({ oldNo: 2, newNo: null, text: 'line2' });
    expect(h0.lines[2]).toMatchObject({ oldNo: null, newNo: 2, text: 'line2 changed' });
    expect(f.hunks[1].lines[1]).toMatchObject({ type: 'add', newNo: 21, text: 'inserted' });
    expect(f.hunks[1].lines[2]).toMatchObject({ type: 'ctx', oldNo: 21, newNo: 22 });
  });

  it('parses several files', () => {
    const two = MULTI + `diff --git a/b.txt b/b.txt
new file mode 100644
index 0000000..3333333
--- /dev/null
+++ b/b.txt
@@ -0,0 +1,2 @@
+hello
+world
`;
    const files = parseDiff(two);
    expect(files.map((f) => f.path)).toEqual(['src/a.ts', 'b.txt']);
    expect(files[1].status).toBe('added');
    expect(files[1].oldPath).toBeNull();
    expect(files[1].added).toBe(2);
    expect(files[1].hunks[0].lines[0]).toMatchObject({ newNo: 1, text: 'hello' });
  });

  it('parses deleted files', () => {
    const [f] = parseDiff(`diff --git a/gone.txt b/gone.txt
deleted file mode 100644
index 4444444..0000000
--- a/gone.txt
+++ /dev/null
@@ -1,2 +0,0 @@
-a
-b
`);
    expect(f.status).toBe('deleted');
    expect(f.path).toBe('gone.txt');
    expect(f.removed).toBe(2);
  });

  it('handles pure renames and renames with edits', () => {
    const files = parseDiff(`diff --git a/old name.txt b/new name.txt
similarity index 100%
rename from old name.txt
rename to new name.txt
diff --git a/x.ts b/y.ts
similarity index 90%
rename from x.ts
rename to y.ts
index 5555555..6666666 100644
--- a/x.ts
+++ b/y.ts
@@ -1 +1 @@
-const a = 1;
+const a = 2;
`);
    expect(files).toHaveLength(2);
    expect(files[0]).toMatchObject({ status: 'renamed', oldPath: 'old name.txt', newPath: 'new name.txt', similarity: 100 });
    expect(files[0].hunks).toHaveLength(0);
    expect(files[1]).toMatchObject({ status: 'renamed', oldPath: 'x.ts', newPath: 'y.ts', similarity: 90, added: 1, removed: 1 });
  });

  it('flags binary files', () => {
    const files = parseDiff(`diff --git a/img.png b/img.png
index 1234567..89abcde 100644
Binary files a/img.png and b/img.png differ
diff --git a/c.txt b/c.txt
index 1..2 100644
--- a/c.txt
+++ b/c.txt
@@ -1 +1 @@
-a
+b
`);
    expect(files[0].binary).toBe(true);
    expect(files[0].hunks).toHaveLength(0);
    expect(files[1].binary).toBe(false);
    expect(files[1].added).toBe(1);
  });

  it('marks no newline at end of file', () => {
    const [f] = parseDiff(`diff --git a/n.txt b/n.txt
index 1..2 100644
--- a/n.txt
+++ b/n.txt
@@ -1 +1 @@
-old
\\ No newline at end of file
+new
\\ No newline at end of file
`);
    const [del, add] = f.hunks[0].lines;
    expect(del.noNewline).toBe(true);
    expect(add.noNewline).toBe(true);
    expect(f.hunks[0].lines).toHaveLength(2);
  });

  it('handles CRLF line endings', () => {
    const crlf = MULTI.replace(/\n/g, '\r\n');
    const [f] = parseDiff(crlf);
    expect(f.hunks).toHaveLength(2);
    expect(f.hunks[0].lines[2].text).toBe('line2 changed');
    expect(f.hunks[0].lines.every((l) => !l.text.includes('\r'))).toBe(true);
  });

  it('does not mistake content lines that look like headers', () => {
    const [f, g] = parseDiff(`diff --git a/m.md b/m.md
index 1..2 100644
--- a/m.md
+++ b/m.md
@@ -1,2 +1,2 @@
--- not a header
+++ also content
 keep
diff --git a/z.txt b/z.txt
--- a/z.txt
+++ b/z.txt
@@ -1 +1 @@
-1
+2
`);
    expect(f.hunks[0].lines).toHaveLength(3);
    expect(f.hunks[0].lines[0]).toMatchObject({ type: 'del', text: '-- not a header' });
    expect(g.path).toBe('z.txt');
  });

  it('treats an omitted count as 1', () => {
    const [f] = parseDiff(`diff --git a/o.txt b/o.txt
--- a/o.txt
+++ b/o.txt
@@ -5 +5 @@
-a
+b
`);
    expect(f.hunks[0]).toMatchObject({ oldStart: 5, oldCount: 1, newStart: 5, newCount: 1 });
  });

  it('records mode changes', () => {
    const [f] = parseDiff(`diff --git a/run.sh b/run.sh
old mode 100644
new mode 100755
`);
    expect(f.modeChange).toContain('100755');
  });
});

describe('toSideBySide', () => {
  it('pairs deletions with following additions', () => {
    const [f] = parseDiff(`diff --git a/s.txt b/s.txt
--- a/s.txt
+++ b/s.txt
@@ -1,4 +1,5 @@
 c
-d1
-d2
+a1
+a2
+a3
 e
`);
    const rows = toSideBySide(f.hunks[0]);
    expect(rows).toHaveLength(5);
    expect(rows[1].left?.text).toBe('d1');
    expect(rows[1].right?.text).toBe('a1');
    expect(rows[3].left).toBeNull();
    expect(rows[3].right?.text).toBe('a3');
    expect(rows[4].left?.text).toBe('e');
  });
});
