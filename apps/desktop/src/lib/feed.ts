// Turns the raw ordered activity list into display rows: tool.call + tool.result are paired.
import type { ActivityEvent, ActivityKind } from './types';

export type FeedItem =
  | { type: 'tool'; key: string; call: ActivityEvent | null; result: ActivityEvent | null }
  | { type: 'event'; key: string; ev: ActivityEvent };

export function buildFeed(events: ActivityEvent[]): FeedItem[] {
  const items: FeedItem[] = [];
  const toolByUse = new Map<string, Extract<FeedItem, { type: 'tool' }>>();
  for (const ev of events) {
    if (ev.kind === 'tool.call') {
      const item: Extract<FeedItem, { type: 'tool' }> = { type: 'tool', key: `e${ev.id}`, call: ev, result: null };
      const id = ev.payload?.toolUseId as string | undefined;
      if (id) toolByUse.set(id, item);
      items.push(item);
    } else if (ev.kind === 'tool.result') {
      const id = ev.payload?.toolUseId as string | undefined;
      const owner = id ? toolByUse.get(id) : undefined;
      if (owner && !owner.result) owner.result = ev;
      else items.push({ type: 'tool', key: `e${ev.id}`, call: null, result: ev });
    } else {
      items.push({ type: 'event', key: `e${ev.id}`, ev });
    }
  }
  return items;
}

export type FilterGroup = 'thinking' | 'tools' | 'files' | 'status';

export function groupOf(kind: ActivityKind): FilterGroup | null {
  switch (kind) {
    case 'agent.thinking':
      return 'thinking';
    case 'tool.call':
    case 'tool.result':
      return 'tools';
    case 'file.changed':
      return 'files';
    case 'status':
    case 'provider.retry':
    case 'provider.rate_limit':
    case 'session.started':
      return 'status';
    default:
      return null;
  }
}

export function filterFeed(items: FeedItem[], hidden: ReadonlySet<FilterGroup>): FeedItem[] {
  if (hidden.size === 0) return items;
  return items.filter((it) => {
    const kind = it.type === 'tool' ? 'tool.call' : it.ev.kind;
    const g = groupOf(kind);
    return !(g && hidden.has(g));
  });
}
