# Offline UI and export fonts

These Noto fonts are bundled for local Unicode PDF export. They are
distributed under the SIL Open Font License 1.1; retain `OFL-Noto.txt` and
`OFL-Noto-CJK.txt` with the fonts. The license files include copyright notices.

Sources: https://github.com/notofonts/noto-fonts and
https://github.com/notofonts/noto-cjk (installed Debian `fonts-noto-core` and
`fonts-noto-cjk` files). `NotoSansCJKsc-Regular.otf` is the unmodified Simplified
Chinese face extracted from the installed collection with FontTools; its
coverage also includes Japanese and Korean. PDFs embed only the glyphs used
in that document. Fonts load
from the packaged app; no CDN request is made when exporting.

The marketing website uses `overtalk/InstrumentSans-Latin.woff2`, a subset of
the existing Instrument Sans variable font. It preserves the Latin and
punctuation needed by the English site, with the same OFL license and variable
weights. The desktop continues to use the complete TTF. Reproduce with FontTools:

```sh
pyftsubset frontend/public/fonts/overtalk/InstrumentSans-Variable.ttf \
  --output-file=frontend/public/fonts/overtalk/InstrumentSans-Latin.woff2 \
  --flavor=woff2 --unicodes='U+0020-007E,U+00A0-00FF,U+2010-203A,U+2190-2199' \
  --layout-features='*'
```

Coverage includes Latin, Greek, Cyrillic, Arabic, Hebrew, Devanagari, Tamil,
Thai and CJK, plus many symbols. Not every Unicode character or emoji has a
glyph. Missing glyphs produce an explicit export error; DOCX and Markdown
preserve arbitrary Unicode text.
