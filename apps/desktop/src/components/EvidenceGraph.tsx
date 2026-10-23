import type { VerificationReport } from '../lib/types';
import { statusLabel, statusTone } from '../lib/verification';

const ROW = 46;
const BOX = 36;

const TONE_STROKE = { ok: '#5fb27a', warn: '#d9a441', bad: '#e5706f', run: '#5ea0ef', idle: '#6c7480' } as const;

/**
 * Requirement -> Check -> Result. Every node and edge comes from the report: criteria, their verifiedBy links,
 * and each check's latest run. Criteria without links get a dashed stub; checks never run get a dashed stub.
 */
export function EvidenceGraph({ report }: { report: VerificationReport }) {
  const checkIndex = new Map(report.checks.map((c, i) => [c.spec.id, i]));
  const names = (id: string) => report.checks[checkIndex.get(id) ?? -1]?.spec.name ?? id;
  const rows = Math.max(report.criteria.length, report.checks.length, 1);
  const height = rows * ROW;
  const cy = (i: number) => i * ROW + ROW / 2;

  // Column x positions as percentages of the width.
  const X = { reqEnd: '31%', checkStart: '36%', checkEnd: '64%', resStart: '69%' };

  return (
    <div className="relative" aria-label="Evidence graph">
      <div className="mb-1 grid grid-cols-[31%_5%_28%_5%_31%] px-0 text-[11px] uppercase tracking-wide text-faint">
        <span>Requirement</span><span /><span>Check</span><span /><span>Result</span>
      </div>
      <div className="relative" style={{ height }}>
        <svg className="absolute inset-0 h-full w-full" aria-hidden>
          {report.criteria.map((k, i) => {
            const linked = k.verifiedBy.filter((id) => checkIndex.has(id));
            if (linked.length === 0) {
              return <line key={k.id} x1={X.reqEnd} y1={cy(i)} x2="34%" y2={cy(i)} stroke="#6c7480" strokeWidth="1.5" strokeDasharray="3 3" />;
            }
            return linked.map((id) => {
              const j = checkIndex.get(id)!;
              const fresh = report.checks[j].fresh && !!report.checks[j].latest;
              return (
                <line key={`${k.id}-${id}`} x1={X.reqEnd} y1={cy(i)} x2={X.checkStart} y2={cy(j)} stroke={TONE_STROKE[statusTone(k.status)]} strokeWidth="1.5" strokeDasharray={fresh ? undefined : '4 3'} opacity={fresh ? 0.9 : 0.4} />
              );
            });
          })}
          {report.checks.map((c, j) => {
            if (!c.latest) return <line key={c.spec.id} x1={X.checkEnd} y1={cy(j)} x2="67%" y2={cy(j)} stroke="#6c7480" strokeWidth="1.5" strokeDasharray="3 3" />;
            const stroke = c.latest.state === 'passed' ? TONE_STROKE.ok : c.latest.state === 'running' ? TONE_STROKE.run : TONE_STROKE.bad;
            return <line key={c.spec.id} x1={X.checkEnd} y1={cy(j)} x2={X.resStart} y2={cy(j)} stroke={stroke} strokeWidth="1.5" strokeDasharray={c.fresh ? undefined : '4 3'} opacity={c.fresh ? 0.9 : 0.4} />;
          })}
        </svg>

        {report.criteria.map((k, i) => {
          const linked = k.verifiedBy.some((id) => checkIndex.has(id));
          const tone = statusTone(k.status);
          return (
            <div key={k.id} className="absolute left-0 flex w-[31%] flex-col justify-center overflow-hidden rounded border border-line-strong bg-raised px-2" style={{ top: i * ROW + (ROW - BOX) / 2, height: BOX }} title={`${k.text}\n${statusLabel(k.status, names)}`}>
              <div className="truncate text-[12px]">{k.text}</div>
              <div className="flex items-center gap-1.5 text-[10.5px]" style={{ color: TONE_STROKE[tone] }}>
                <span className="truncate">{statusLabel(k.status, names)}</span>
                {!linked && <span className="shrink-0 text-faint">· {k.manualVerdict ? 'manual verdict' : k.scopeRule ? 'scope rule' : 'no evidence'}</span>}
              </div>
            </div>
          );
        })}

        {report.checks.map((c, j) => (
          <div key={c.spec.id} className="absolute left-[36%] flex w-[28%] flex-col justify-center overflow-hidden rounded border border-line-strong bg-raised px-2" style={{ top: j * ROW + (ROW - BOX) / 2, height: BOX }} title={c.spec.command}>
            <div className="truncate text-[12px] font-medium">{c.spec.name}</div>
            <div className="mono truncate text-[10.5px] text-faint">{c.spec.command}</div>
          </div>
        ))}

        {report.checks.map((c, j) => {
          const r = c.latest;
          const stale = !!r && !c.fresh;
          const tone = !r ? 'idle' : r.state === 'passed' ? 'ok' : r.state === 'running' ? 'run' : 'bad';
          return (
            <div
              key={c.spec.id}
              className={`absolute left-[69%] flex w-[31%] flex-col justify-center overflow-hidden rounded border px-2 ${r ? 'bg-raised' : 'bg-transparent'} ${stale || !r ? 'border-dashed opacity-70' : ''}`}
              style={{ top: j * ROW + (ROW - BOX) / 2, height: BOX, borderColor: TONE_STROKE[tone] }}
              title={r ? `${c.spec.name}: ${r.state}${r.exitCode !== null ? `, exit ${r.exitCode}` : ''}${stale ? ' (ran against earlier changes)' : ''}` : `${c.spec.name}: never run`}
            >
              {r ? (
                <>
                  <div className="text-[12px]" style={{ color: TONE_STROKE[tone] }}>{r.state}{r.exitCode !== null ? ` · exit ${r.exitCode}` : ''}</div>
                  <div className="truncate text-[10.5px] text-faint">{stale ? 'stale: earlier changes' : 'current changes'}</div>
                </>
              ) : (
                <div className="text-[12px] text-faint">never run</div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
