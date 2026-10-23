/** Parses the allow-list field: comma/space/newline separated tool names (the part after `mcp__<server>__`). */
export function parseAllowlist(text: string): string[] {
  const out: string[] = [];
  for (const raw of text.split(/[\s,]+/)) {
    const t = raw.trim().replace(/^mcp__[^_]+(?:_[^_]+)*__/, '');
    if (t && !out.includes(t)) out.push(t);
  }
  return out;
}
