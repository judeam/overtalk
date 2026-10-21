import type { PatternBin } from '@/lib/session';
import { toneOf } from '@/lib/session';

const WIDTH = 206;
const HEIGHT = 40;

/** Who spoke when across the whole meeting: one bar per slice, tinted by the main speaker. */
export function SpeakingPattern({ pattern }: { pattern: PatternBin[] }) {
  const pitch = pattern.length ? (WIDTH - 12) / pattern.length : 0;
  const bar = Math.max(1, pitch * 0.5);
  return (
    <svg className="ot-pattern" viewBox={`0 0 ${WIDTH} ${HEIGHT}`} preserveAspectRatio="none" aria-hidden="true">
      {pattern.length > 0 && <rect x={6} y={HEIGHT / 2 - 0.4} width={WIDTH - 12} height={0.8} fill="var(--ot-line-strong)" />}
      {pattern.map((bin, index) => {
        const height = bin.speaker < 0 ? 1 : Math.max(3, (0.2 + 0.8 * bin.level) * (HEIGHT - 12));
        return (
          <rect
            key={index}
            x={6 + index * pitch}
            y={(HEIGHT - height) / 2}
            width={bar}
            height={height}
            fill={bin.speaker < 0 ? 'var(--ot-line-strong)' : `var(--ot-${toneOf(bin.speaker)}-wave)`}
          />
        );
      })}
    </svg>
  );
}
