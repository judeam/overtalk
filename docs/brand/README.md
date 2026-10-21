# Overtalk identity

Overtalk is a local meeting notebook. Its overlapping speech shapes refer to
people talking over one another; speaker lanes make those voices visible.

## Mark and source files

The approved mark is variation **17b**, with a teal speech shape and a pink speech
shape separated by an open seam. The web component is
[`BrandMark.tsx`](../../frontend/src/components/brand/BrandMark.tsx); the desktop
icon source is [`icon.svg`](../../frontend/public/brand/icon.svg). Native assets
live in [`native/assets/`](../../native/assets/). Do not substitute earlier
concept sheets for the current mark.

## Colour and type

The native [theme](../../native/ui/theme.slint) is authoritative for interface
tokens. Dark and light modes use different contrast values. Teal identifies
primary actions; speaker colours run teal, pink, slate, moss, clay and plum.
Amber identifies decisions, violet actions, and red the recording/playhead.
The icon uses teal `#0F9A9F` and pink `#E0457B` on `#10151B`.

Instrument Sans carries reading text and labels; Phudu carries the transport
timecode. Both fonts are self-hosted under
[`frontend/public/fonts/overtalk/`](../../frontend/public/fonts/overtalk/) with
their SIL Open Font License files.

## Product media

[README screenshots and the demo](../media/README.md) show actual native widgets
and synthetic content. Earlier generated design concepts are references, not
screenshots of working software. No trademark clearance is implied.
