import { create } from 'fontkit';
import type { Font, Glyph, SubsetStream } from 'pdf-lib/es/types/fontkit';

type CurrentFont = Omit<Font, 'createSubset'> & {
  createSubset(): { includeGlyph(glyph: Glyph): number; encode(): Uint8Array; cff?: unknown };
};

/** Adapt fontkit 2's synchronous subset encoder to pdf-lib's stream interface. */
export const pdfFontkit = {
  create(bytes: Uint8Array, name?: string): Font {
    const font = create(bytes, name) as CurrentFont;
    const originalSubset = font.createSubset.bind(font);
    const adapted = font as unknown as Font;
    adapted.createSubset = () => {
      const subset = originalSubset();
      return {
        cff: subset.cff,
        includeGlyph: subset.includeGlyph.bind(subset),
        encodeStream(): SubsetStream {
          const callbacks: { data?: (bytes: Uint8Array) => void; end?: () => void; error?: (error: Error) => void } = {};
          const stream: SubsetStream = {
            on(event, callback) {
              if (event === 'data') callbacks.data = callback;
              else if (event === 'end') callbacks.end = callback as unknown as () => void;
              else callbacks.error = callback as unknown as (error: Error) => void;
              return stream;
            },
          };
          queueMicrotask(() => {
            try { callbacks.data?.(subset.encode()); callbacks.end?.(); }
            catch (error) { callbacks.error?.(error instanceof Error ? error : new Error(String(error))); }
          });
          return stream;
        },
      };
    };
    return adapted;
  },
};
