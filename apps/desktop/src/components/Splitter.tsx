import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { readNumber, writeStore } from '../lib/storage';

interface Props {
  storageKey: string;
  /** Which pane has the persisted fixed width; the other flexes. */
  fixed: 'start' | 'end';
  defaultSize: number;
  min: number;
  max: number;
  children: [ReactNode, ReactNode];
}

/** Two horizontal panes with a draggable, keyboard-operable divider. Size persists in localStorage. */
export function SplitLayout({ storageKey, fixed, defaultSize, min, max, children }: Props) {
  const [size, setSize] = useState(() => clamp(readNumber(`forge.split.${storageKey}`, defaultSize), min, max));
  const ref = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);

  const commit = useCallback(
    (n: number) => {
      const v = clamp(n, min, max);
      setSize(v);
      writeStore(`forge.split.${storageKey}`, String(Math.round(v)));
    },
    [min, max, storageKey],
  );

  useEffect(() => {
    const move = (e: PointerEvent) => {
      if (!dragging.current || !ref.current) return;
      const r = ref.current.getBoundingClientRect();
      const total = r.width;
      const containerMax = Math.min(max, total - 280);
      const raw = fixed === 'end' ? r.right - e.clientX : e.clientX - r.left;
      setSize(clamp(raw, min, containerMax));
    };
    const up = () => {
      if (!dragging.current) return;
      dragging.current = false;
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      setSize((s) => {
        writeStore(`forge.split.${storageKey}`, String(Math.round(s)));
        return s;
      });
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
    return () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
    };
  }, [fixed, min, max, storageKey]);

  const onKey = (e: React.KeyboardEvent) => {
    const step = e.shiftKey ? 60 : 20;
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    const move = (e.key === 'ArrowLeft' ? -1 : 1) * step; // separator movement in px
    commit(size + (fixed === 'end' ? -move : move));
    e.preventDefault();
  };

  const startStyle = fixed === 'start' ? { width: size, flex: 'none' as const } : { flex: '1 1 0', minWidth: 0 };
  const endStyle = fixed === 'end' ? { width: size, flex: 'none' as const } : { flex: '1 1 0', minWidth: 0 };

  return (
    <div ref={ref} className="flex h-full min-h-0 w-full min-w-0">
      <div className="flex min-h-0 min-w-0 flex-col" style={startStyle}>
        {children[0]}
      </div>
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize panel"
        aria-valuenow={Math.round(size)}
        aria-valuemin={min}
        aria-valuemax={max}
        tabIndex={0}
        onKeyDown={onKey}
        onDoubleClick={() => commit(defaultSize)}
        onPointerDown={(e) => {
          e.preventDefault();
          dragging.current = true;
          document.body.style.cursor = 'col-resize';
          document.body.style.userSelect = 'none';
        }}
        className="group relative w-[5px] shrink-0 cursor-col-resize bg-line transition-colors hover:bg-accent/60 focus-visible:bg-accent"
      >
        <span className="absolute inset-y-0 -left-1 -right-1" />
      </div>
      <div className="flex min-h-0 min-w-0 flex-col" style={endStyle}>
        {children[1]}
      </div>
    </div>
  );
}

function clamp(n: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, n));
}
