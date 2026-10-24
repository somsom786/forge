import { useCallback, useRef, useState, type ReactNode } from 'react';
import * as Dialog from '@radix-ui/react-dialog';

/** The app's styled modal (role=dialog). Used instead of native confirm/alert/prompt. */
export function Modal({
  open, onClose, title, children, footer, tone, wide,
}: { open: boolean; onClose: () => void; title: string; children: ReactNode; footer?: ReactNode; tone?: 'bad'; wide?: boolean }) {
  // A body that scrolls but holds nothing focusable (for example only disabled buttons) must itself be focusable, or
  // keyboard users cannot scroll it. Otherwise it stays out of the tab order so the first control gets focus as usual.
  const [scrollFocus, setScrollFocus] = useState(false);
  const observers = useRef<{ disconnect(): void }[]>([]);
  const body = useCallback((el: HTMLDivElement | null) => {
    observers.current.forEach((o) => o.disconnect());
    observers.current = [];
    if (!el) return;
    // Its own focusability does not count: only content that can take focus.
    const check = () => setScrollFocus(el.scrollHeight > el.clientHeight && !el.querySelector('a[href], button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])'));
    check();
    // Re-check when the body resizes and when its content changes (controls added, enabled or disabled).
    if (typeof ResizeObserver !== 'undefined') {
      const r = new ResizeObserver(check);
      r.observe(el);
      observers.current.push(r);
    }
    if (typeof MutationObserver !== 'undefined') {
      const m = new MutationObserver(check);
      m.observe(el, { subtree: true, childList: true, attributes: true, attributeFilter: ['disabled', 'tabindex', 'href'] });
      observers.current.push(m);
    }
  }, []);
  return (
    <Dialog.Root open={open} onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-[60] bg-black/65" />
        <Dialog.Content
          aria-describedby={undefined}
          className={`fixed left-1/2 top-1/2 z-[60] flex max-h-[88vh] -translate-x-1/2 -translate-y-1/2 flex-col rounded-lg border bg-panel shadow-2xl shadow-black/60 ${wide ? 'w-[760px]' : 'w-[560px]'} max-w-[94vw] ${tone === 'bad' ? 'border-bad/40' : 'border-line-strong'}`}
        >
          <div className="border-b border-line px-4 py-3">
            <Dialog.Title className="m-0 text-[14px] font-semibold">{title}</Dialog.Title>
          </div>
          <div ref={body} className="min-h-0 flex-1 space-y-3 overflow-y-auto p-4" {...(scrollFocus ? { tabIndex: 0, role: 'region', 'aria-label': title } : {})}>{children}</div>
          {footer && <div className="flex justify-end gap-2 border-t border-line px-4 py-3">{footer}</div>}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
