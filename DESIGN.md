---
name: Overtalk
description: A local-first meeting notebook that lays a meeting out as a multitrack session.
colors:
  ground: "#f1f4f6"
  chrome: "#f3f6f8"
  rail: "#fafcfd"
  panel: "#ffffff"
  lane: "#e9eef1"
  lane-head: "#f8f9fb"
  grid: "#dce2e7"
  line: "#e1e6ea"
  line-strong: "#cfd6dc"
  select: "#e9eff5"
  select-strong: "#dbe3eb"
  key: "#e8ecef"
  ink: "#151a21"
  ink-2: "#3c4552"
  ink-3: "#5f6977"
  ink-hover: "#262e39"
  on-ink: "#ffffff"
  red: "#e5322d"
  danger: "#b42318"
  warn: "#8a5a00"
  teal: "#0f9a9f"
  teal-fill: "#a8d9da"
  teal-wave: "#10898e"
  magenta: "#e0457b"
  magenta-fill: "#eeb3cd"
  magenta-wave: "#d6306f"
  slate: "#5b6b86"
  slate-fill: "#b4bdcd"
  slate-wave: "#46556f"
  moss: "#5e8c31"
  moss-fill: "#c4dba9"
  moss-wave: "#4b7424"
  clay: "#c4692f"
  clay-fill: "#f0c6a6"
  clay-wave: "#a95520"
  plum: "#8a4f9e"
  plum-fill: "#d7bfe0"
  plum-wave: "#74408a"
  amber: "#efb502"
  amber-fill: "#fde189"
  amber-edge: "#e8bd22"
  violet: "#9068cd"
  violet-fill: "#c4b0ee"
  violet-edge: "#b39be6"
typography:
  timecode:
    fontFamily: "Phudu, Instrument Sans, sans-serif"
    fontSize: "calc(34 * var(--u))"
    fontWeight: 500
    lineHeight: 1
    letterSpacing: "-0.03em"
    fontFeature: "'tnum' 1"
  headline:
    fontFamily: "Instrument Sans, Segoe UI, sans-serif"
    fontSize: "calc(20.8 * var(--u))"
    fontWeight: 600
    lineHeight: "calc(28 * var(--u))"
    letterSpacing: "-0.005em"
    fontVariation: "'wdth' 92"
  quote:
    fontFamily: "Instrument Sans, Segoe UI, sans-serif"
    fontSize: "calc(20.8 * var(--u))"
    fontWeight: 400
    lineHeight: "calc(29 * var(--u))"
    fontVariation: "'wdth' 94"
  title:
    fontFamily: "Instrument Sans, Segoe UI, sans-serif"
    fontSize: "calc(18 * var(--u))"
    fontWeight: 600
    lineHeight: 1.3
    fontVariation: "'wdth' 92"
  body:
    fontFamily: "Instrument Sans, Segoe UI, sans-serif"
    fontSize: "calc(18 * var(--u))"
    fontWeight: 400
    lineHeight: "calc(24 * var(--u))"
    letterSpacing: "-0.015em"
    fontVariation: "'wdth' 92"
  meta:
    fontFamily: "Instrument Sans, Segoe UI, sans-serif"
    fontSize: "calc(15 * var(--u))"
    fontWeight: 400
    lineHeight: "calc(20 * var(--u))"
    fontVariation: "'wdth' 100"
  label:
    fontFamily: "Instrument Sans, Segoe UI, sans-serif"
    fontSize: "calc(14 * var(--u))"
    fontWeight: 400
    lineHeight: 1.3
    fontVariation: "'wdth' 100"
  button:
    fontFamily: "Instrument Sans, Segoe UI, sans-serif"
    fontSize: "calc(18 * var(--u))"
    fontWeight: 500
    lineHeight: 1
    fontVariation: "'wdth' 92"
rounded:
  clip: "calc(4 * var(--u))"
  button: "calc(5 * var(--u))"
  row: "calc(6 * var(--u))"
  panel: "calc(8 * var(--u))"
  card: "calc(10 * var(--u))"
  round: "50%"
spacing:
  hair: "calc(2 * var(--u))"
  xs: "calc(4 * var(--u))"
  sm: "calc(8 * var(--u))"
  md: "calc(12 * var(--u))"
  lg: "calc(16 * var(--u))"
  inset: "calc(23 * var(--u))"
  deck: "calc(26 * var(--u))"
components:
  button-primary:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.on-ink}"
    typography: "{typography.button}"
    rounded: "{rounded.button}"
    padding: "0 calc(18 * var(--u))"
    height: "calc(46 * var(--u))"
  button-primary-hover:
    backgroundColor: "{colors.ink-hover}"
    textColor: "{colors.on-ink}"
  button-primary-disabled:
    backgroundColor: "{colors.key}"
    textColor: "{colors.ink-3}"
  button-secondary:
    backgroundColor: "{colors.chrome}"
    textColor: "{colors.ink}"
    typography: "{typography.button}"
    rounded: "{rounded.button}"
    padding: "0 calc(18 * var(--u))"
    height: "calc(46 * var(--u))"
  button-secondary-hover:
    backgroundColor: "{colors.key}"
  button-small:
    rounded: "{rounded.button}"
    padding: "0 calc(14 * var(--u))"
    height: "calc(36 * var(--u))"
  icon-button:
    textColor: "{colors.ink-2}"
    rounded: "{rounded.panel}"
    size: "calc(34 * var(--u))"
  icon-button-hover:
    backgroundColor: "{colors.key}"
    textColor: "{colors.ink}"
  deck-button:
    backgroundColor: "{colors.chrome}"
    textColor: "{colors.ink}"
    rounded: "{rounded.round}"
    size: "calc(46 * var(--u))"
  deck-button-hover:
    backgroundColor: "{colors.key}"
  tool:
    textColor: "{colors.ink-2}"
    rounded: "{rounded.row}"
    padding: "0 calc(10 * var(--u))"
    height: "calc(34 * var(--u))"
  tool-hover:
    backgroundColor: "{colors.chrome}"
    textColor: "{colors.ink}"
  lane-key:
    backgroundColor: "{colors.key}"
    textColor: "{colors.ink-2}"
    typography: "{typography.label}"
    rounded: "{rounded.clip}"
    size: "calc(22 * var(--u))"
  lane-key-pressed:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.on-ink}"
  lane:
    backgroundColor: "{colors.lane}"
    rounded: "{rounded.row}"
    height: "calc(74 * var(--u))"
  lane-head:
    backgroundColor: "{colors.lane-head}"
    typography: "{typography.title}"
    width: "calc(140 * var(--u))"
  clip-teal:
    backgroundColor: "{colors.teal-fill}"
    textColor: "{colors.teal-wave}"
    rounded: "{rounded.clip}"
  note-decision:
    backgroundColor: "{colors.amber-fill}"
    textColor: "{colors.ink}"
    rounded: "{rounded.button}"
    padding: "0 calc(14 * var(--u))"
  note-action:
    backgroundColor: "{colors.violet-fill}"
    textColor: "{colors.ink}"
    rounded: "{rounded.button}"
    padding: "0 calc(14 * var(--u))"
  transcript-line:
    textColor: "{colors.ink}"
    typography: "{typography.body}"
    rounded: "{rounded.row}"
    padding: "calc(15 * var(--u)) calc(12 * var(--u)) calc(13 * var(--u))"
  transcript-line-current:
    backgroundColor: "{colors.select}"
  meeting-item:
    rounded: "{rounded.card}"
    padding: "calc(8 * var(--u)) calc(14 * var(--u)) 0"
    height: "calc(112 * var(--u))"
  meeting-item-current:
    backgroundColor: "{colors.select}"
  panel:
    backgroundColor: "{colors.panel}"
    rounded: "{rounded.panel}"
  input:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.ink}"
    rounded: "{rounded.row}"
    padding: "0 calc(10 * var(--u))"
    height: "calc(36 * var(--u))"
  menu-item:
    textColor: "{colors.ink}"
    typography: "{typography.meta}"
    rounded: "{rounded.button}"
    padding: "calc(6 * var(--u)) calc(10 * var(--u))"
    height: "calc(36 * var(--u))"
---

# Design System: Overtalk

## Implementation authority

The current desktop application is native Slint/Rust. `native/ui/theme.slint`
defines live colors and scaled typography; `native/ui/app.slint` and
`native/ui/sidebar.slint` own the shell. `meeting.slint`, `timeline.slint`,
`notes.slint`, `inspector.slint`, `projects.slint` and `workspace.slint` implement
the corresponding surfaces. The Rust controller lives in `native/src/controller/`.
The retained `frontend/` Next.js/React/Tauri implementation is a historical visual
reference and alternative frontend. CSS class names and CSS measurements below
record that reference, not executable authority for the native desktop app.
Preserve the palette, type, hierarchy and semantics when adapting to native layout.

Native windows fill the available area from 760×540 upward. Navigation becomes
a dismissible overlay below 1100 px; it must not squeeze the page. Short windows
reserve readable meeting content and complete project rows. The Notes timeline
is a compact resizable overview; the Transcript timeline remains expanded when
height permits. Project details are top-aligned beside the list on wide windows,
with local commands, and use a dismissible inspector on narrow windows. Activity
commands reflect job state; secondary destinations have visible names.

## Overview

**Creative North Star: "The Multitrack Session"**

Overtalk lays a meeting out the way an audio editor lays out a recording: a transport bar with a clock face across the top, one lane per speaker on a shared time ruler, decisions and actions as their own lanes of clips, a single red playhead, and below it the transcript as a lyric sheet beside an inspector for whatever moment is selected. The interface is the meeting's own data; there is no decoration that is not a clip, a lane, a line or a control.

The world is cool, light and graphite: a pale grey-green ground, white panels, hairline rules and near-black ink. Colour belongs to content. Speakers own fixed clip colours, decisions are amber, actions are violet, and the one red marks recording and the playhead. Controls stay in ink and greys so the lanes carry the colour. Density is that of a desktop tool used beside a video call: compact, with every size authored at a 1536 px reference frame and scaled by one unit.

Retained web reference: `frontend/src/styles/overtalk.css` records its tokens, components, responsive rules and motion. In that frontend, settings, library and dialogs read the palette through the shadcn HSL bridge in `frontend/src/app/globals.css`. A dark scheme mirrors the light palette for use at night beside a dark call window. Its Tauri window keeps native decorations (`frontend/src-tauri/tauri.conf.json`, `decorations: true`); native Slint implementation authority is defined above.

**Key Characteristics:**
- One timeline: speakers, decisions and actions are lanes of clips on one ruler, with a single red playhead.
- Colour is data: six fixed speaker tones, amber for decisions, violet for actions; chrome stays ink and grey.
- Primary actions are ink-filled. No blue anywhere.
- Flat tonal layers and 1 px hairlines; shadows only lift panels and floating surfaces.
- Instrument Sans, slightly condensed, for everything read; Phudu only for the transport clock.
- Every session size is `calc(n * var(--u))` at a 1536 px frame.

## Colors

A cool graphite-on-pale-grey workspace in which colour is reserved for what was said and decided.

### Primary
- **Graphite Ink** (`ink`): text, the fill of every primary action ("Play from 03:09", "Get started", "Mark this moment"), the selected-tab underline, pressed lane keys, focus outlines, mark pins and the overlap in the mark. Hover deepens to `ink-hover`. In dark mode ink inverts to near-white and `on-ink` to the ground, so primary buttons stay the highest-contrast object.
- **Ink Two / Ink Three** (`ink-2`, `ink-3`): secondary text (icon buttons, ruler labels, transcript times) and tertiary text (meta lines, placeholders, field labels, empty states).

### Secondary
- **Speaker tones** (`teal`, `magenta`, `slate`, `moss`, `clay`, `plum`): each has a solid (lane chip, transcript dot, owner dot), a `-fill` (clip body) and a `-wave` (waveform stroke, pressed-clip ring, clip hairline at 22 % mix). They are assigned in order of first appearance by `SPEAKER_TONES` in `frontend/src/lib/session.ts` and cycle after six. Applied through `.ot-tone-*` classes that set `--tone`, `--tone-fill`, `--tone-wave`.

### Tertiary
- **Decision Amber** (`amber`, `amber-fill`, `amber-edge`): the Decisions lane chip and decision clips (fill with a 2u inset edge).
- **Action Violet** (`violet`, `violet-fill`, `violet-edge`): the Actions lane chip and action clips (fill with a 1u inset edge; pressed takes a 2u `violet` ring).
- **Record Red** (`red`): the record dot, the live indicator and its breathing halo, the recording-start spinner, the recording alert edge, and the playhead with its cap.

### Neutral
- **Ground** (`ground`): the app background and the sticky ruler.
- **Chrome** (`chrome`): header bar, hover fill for rows and tools, secondary buttons, deck buttons.
- **Rail** (`rail`): the meeting list column.
- **Panel** (`panel`): transcript sheet, inspector, notes, menus, inputs, transport bar.
- **Lane / Lane Head / Grid** (`lane`, `lane-head`, `grid`): track bodies, their name column, and the 1 px vertical grid lines drawn across tracks.
- **Line / Line Strong** (`line`, `line-strong`): hairline borders and dividers; strong for input borders, ticks, scrollbar thumbs, dashed pending lanes.
- **Select / Select Strong** (`select`, `select-strong`): current meeting, current transcript line; strong for key hover and the line-arrival flash.
- **Key** (`key`): lane S/M keys, icon-button hover and expanded state, disabled primary.
- **Danger / Warn** (`danger`, `warn`): destructive menu items and error and warning text only.

Current dark values live in `native/ui/theme.slint`; the retained web reference uses the `prefers-color-scheme: dark` and `:root.dark` blocks of `overtalk.css` (ground `#0f1318`, panel `#161b22`, ink `#e8ecf0`, red `#f0483f`; speaker and note fills darken, waves lighten). They are recorded per token in `.impeccable/design.json`.

**Legacy bridge.** In the retained web frontend, `globals.css` maps shadcn's HSL variables onto the same palette: `--background` is the ground, `--primary` and `--ring` are ink, `--card` is white, `--destructive` is a red-brown close to `danger`. Its settings, library and dialogs inherit Overtalk through this layer.

### Named Rules
**The One Red Rule.** Red marks recording and the playhead, nothing else. It is never a hover, a badge, an error tint for unrelated features, or an accent.

**The Ink Action Rule.** Primary actions are ink-filled with `on-ink` text (decided 14 Oct 2026: "Ink, no blue"). There is no blue in the system: no blue links, focus rings, selections or accents. Slate is a desaturated speaker tone and the cool neutrals are greys; neither stands in for a blue accent.

**The Lane Colour Rule.** Speakers take teal, magenta, slate, moss, clay, plum in order of first appearance. Amber means a decision and violet an action; neither is ever given to a speaker, and speaker tones never mark notes.

## Typography

**Body Font:** Instrument Sans variable (SIL OFL; wght 400 to 700, wdth 75 to 100), loaded as `--font-instrument`, fallback Segoe UI, sans-serif.
**Display Font:** Phudu variable (SIL OFL), loaded as `--font-phudu`, transport timecode only.

Both are bundled from `frontend/public/fonts/overtalk/` with their licences and imported directly by `native/ui/app.slint`. The retained web frontend loads them in `frontend/src/app/layout.tsx`.

**Character:** a neutral grotesk set slightly narrow (`wdth` 92 by default, 94 for quotes and notes, 100 for small meta and labels so they stay open at small sizes), paired with a compact squared display face that makes the clock read like hardware.

### Hierarchy
- **Timecode** (Phudu 500, 34u, line-height 1, -0.03em): the transport clock `00:03:09`. 28u under 900 px.
- **Headline** (600, 20.8u, 28u): the editable meeting title in the transport. First run uses its own fixed scale (600, `clamp(28px, 4vw, 40px)`, 1.1, -0.025em).
- **Quote** (400, 20.8u, 29u, wdth 94): the selected moment in the inspector, hanging-indented opening quote mark, max 285u wide.
- **Title** (600, 16.7u to 19.5u): brand name 19.5u, rail title 18u, tabs (selected 600, unselected 400 in `ink-3`) 18u, inspector title 18u, lane names 17.5u, meeting titles 16.7u.
- **Body** (400, 18u, 24u, wdth 92, -0.015em): transcript lines; notes document 18u at 1.6, max 76ch.
- **Meta** (400, 15u, 20u, wdth 100, `ink-3`): dates, durations, speaker summaries, menu items, small buttons.
- **Label** (400, 14u, wdth 100): ruler labels, lane keys, context `dt` labels, lane sub-lines.

Every time value is set with tabular numerals (`.ot-time`, `font-variant-numeric: tabular-nums`); transcript times add 0.06em tracking. Formats come from `session.ts`: `timecode()` is `HH:MM:SS` for the transport, `clock()` is `MM:SS` or `H:MM:SS` elsewhere, `shortDate()` is en-GB "12 Oct".

### Named Rules
**The Clock Face Rule.** Phudu appears once per screen, in the transport timecode. Every other time, heading and label is Instrument Sans.

**The Condensed Reading Rule.** Reading text sits at `wdth` 90 to 94; only small meta and labels open to 100.

## Layout

**Reference frame.** `--u: clamp(0.8px, calc(100vw / 1536), 1.12px)`. Every session measurement is `calc(n * var(--u))`, so the 1536 px composition keeps its proportions from the default 1100 px window up to large displays. First run fixes `--u: 1px`.

**Shell** (`native/ui/app.slint`, retained web reference `.ot-app`): a two-column grid, rail 255u and main column, under a 46u header carrying the mark, the name and the tagline.

**Session** (`.ot-session`): three rows: transport 80u; the arrangement (auto, max 58dvh, scrolls with a sticky 32u ruler); the lower deck (`.ot-deck-lower`), transcript sheet beside a 450u inspector with a 12u gap. In notes view the lower deck becomes one column.

**Arrangement** (`native/ui/timeline.slint`; retained reference `Arrangement.tsx`): lanes are a grid of a 140u head (168u while live) and a track; lane height 74u (88u live), lane gap 8u. Tracks draw a 1 px grid at `--grid-step`; the ruler carries minor and major ticks from `rulerSteps()`.

**Transcript line** (`native/ui/meeting.slint`; retained reference `TranscriptSheet.tsx`): a four-column grid, time 76u, dot 33u, speaker 111u, text; baseline aligned, 15u between lines.

**Rhythm.** Small steps of 2, 4, 6, 8, 10, 12, 14, 16u inside components; panels inset 22 to 23u; transport and deck groups space at 26u.

**Responsive.**
- At 900 px and below: the rail becomes a drawer (min(290u, 86vw)) toggled from the header with a scrim; the tagline hides; transport compacts (deck buttons 40u, timecode 28u, source names hidden); lane head 124u, lane 62u, gap 6u, arrangement max 46dvh; the lower deck stacks, inspector and marks cap at 34dvh and the inspector drops its context block; transcript columns tighten to 62/24/92u.
- At 700 px and below: the meta line keeps only its first item; transcript lines become two columns, time then speaker over text, and the dot hides.
- Short windows (height 820 px and below, width above 900 px): lanes 64u with 6u gaps and 13u keys.


## Full-window workspace (current)

The application fills its native window; do not reintroduce a centered fixed-width app frame. Native typography and workspace dimensions use `Theme.scale` so controls grow with the text; the width-derived scale is combined with the user's zoom in `native/ui/app.slint`. Paragraphs retain a readable line length inside the full-width layout.

The timeline opens by default. Auto fit shows every lane when screen height permits, while reserving a usable transcript area on short screens. The user can resize or hide it. Below 1440 px the selected moment opens separately, leaving more room for the transcript. Teal identifies active controls and transcript context, amber identifies decisions, and violet identifies actions and notes.

## Elevation & Depth

Depth is tonal first: ground, rail, chrome, lane and panel are separate steps, divided by 1 px `line` hairlines. Shadows are soft and graphite-tinted, and appear only on panels and on surfaces that float above the session.

### Shadow Vocabulary
- **Panel ambient** (`--ot-shadow`: `0 1u 2u rgb(21 26 33 / 0.05), 0 2u 8u rgb(21 26 33 / 0.04)`): transcript sheet, inspector, notes, first-run preview, the meeting row's revealed more button. Dark: `0 1px 2px rgb(0 0 0 / 0.4)`.
- **Deck button** (`0 1u 2u rgb(21 26 33 / 0.06)`): transport circles.
- **Primary lift** (`0 1u 2u rgb(21 26 33 / 0.18)`): ink buttons.
- **Floating** (`0 8u 28u rgb(21 26 33 / 0.14)`): menus; the recording alert uses `0 8u 24u` at the same alpha.
- **Drawer** (`0 0 40u rgb(21 26 33 / 0.18)`): the rail when open as a drawer.
- **Pressed note glow** (`0 2u 8u`, amber `rgb(160 120 0 / 0.18)` or violet `rgb(90 50 160 / 0.16)`): a selected decision or action clip.

Clips and inputs use inset rings (`box-shadow: inset 0 0 0 1px ...`) for edges, not drop shadows.

### Named Rules
**The Hairline Rule.** Structure is drawn with 1 px lines and tonal steps. A shadow means the surface is a panel or is floating; lanes, rows and clips never cast one at rest.

## Shapes

Soft-cornered rectangles throughout, in a narrow radius band: 4u for clips, chips and keys; 5u for buttons and note clips; 6u for lanes, transcript lines and inputs; 8u for panels, menus, icon buttons and the search field; 10u for meeting rows. Full circles are reserved for the three transport deck buttons, the record dot, speaker dots and the playhead cap. The record dot squares to a 4u-radius stop shape while recording. Mark pins are a pentagon flag (`clip-path`). The selected 17b mark uses two curved speech shapes; preserve the supplied SVG geometry. Pending lanes use a dashed `line-strong` border.

## Components

### Buttons
- **Shape:** gently squared (5u), height 46u, padding 0 18u, 18u weight 500, icon 20u with a 14u gap.
- **Primary:** ink fill, `on-ink` text, primary lift shadow. Hover `ink-hover`; disabled becomes `key` with `ink-3` text and no shadow. Keyboard hints sit in a translucent `kbd` inside the button.
- **Secondary:** chrome fill with an inset 1 px `line-strong` ring; hover `key`.
- **Press:** `translateY(1px)`.
- **Small:** 36u high, padding 0 14u, 15u text.
- **Notes toolbar** (`.ot-notes-actions`): one quiet row of transparent 34u buttons in `ink-2`; the single save action is ink-filled and ordered first; unsaved state shows an inset ink ring.

### Icon buttons and tools
- **Icon button:** 34u square, 8u radius, `ink-2`; hover or expanded fills `key`; `aria-current` page fills `key`. Icons 22u at stroke 1.75 (Lucide).
- **Tool:** a text-and-icon button, 34u high, 6u radius, 15u `ink-2`; hover chrome.

### Transport (`native/ui/meeting.slint`)
White bar 80u high: three 46u circular deck buttons (record, play, stop) on chrome with a hairline border (press scales to 0.96), the Phudu timecode, a 44u vertical divider, then the editable title over a meta line, and tools at the right. While live the record button becomes the red live indicator with a breathing halo, and source pickers (microphone, system) sit as hairline-joined segments.

### Arrangement, lanes and clips (`native/ui/timeline.slint`)
- **Lane:** lane fill, hairline border, 6u radius. The head holds a 20u tone chip, the name, and S/M keys (22u, `key`; pressed fills ink). A muted lane drops its clips to 0.38 opacity.
- **Speech clip:** tone fill with a waveform in the tone wave colour, inset hairline at 22 % wave; hover saturates slightly; pressed takes a 2u wave ring. Clips merge a speaker's turns across pauses of 1.5 s or less (`speechClips()`). The newest live clip fades out toward the record head; silent clips show a flat centre line.
- **Note clips:** decisions are amber fill with a 2u amber edge, two lines; actions are violet fill with a 1u edge, inset 4u further and one line. Clips sit above the playhead (z-index 4) so text stays legible. They are derived by `noteClips()` from list items under decision or action headings in the notes, placed at the moments they cite as `[Sn]` or at a written timestamp; items with neither get no clip.
- **Playhead:** a 2u red line with a 10u round cap on the ruler; the only red in a review session.
- **Mark pin:** an ink pentagon flag at the moment marked while recording; hover or press lifts it 2u.
- **Pending lane:** dashed border, translucent fill, grey chip and grey clips while speakers are still unresolved.

### Transcript sheet (`native/ui/meeting.slint`)
A panel with tabs (Transcript, Speakers): unselected `ink-3`, selected ink, weight 600 and a 3u ink underline. Lines are rows: time, tone dot, speaker in 600, text; hover chrome, current line `select`; partial live text in `ink-3`. Editing swaps the text for a bordered textarea.

### Inspector (`native/ui/inspector.slint`)
The selected moment: kind and time as a 600 title with a dot separator, the quote, owner with tone dot, a hairline, then the primary "Play from" and a secondary "Copy" in a 242u and 1fr grid, then a context definition list (127u labels). While recording, the marks panel takes this slot, headed "Marked moments" and led by a full-width ink "Mark this moment" button with its key hint.

### Meeting rail (`native/ui/sidebar.slint`)
Rows 112u, 10u radius: title 600, meta in `ink-3` with dot separators, and a 206u by 40u speaking-pattern thumbnail in speaker tones. Hairline separators between rows hide around the current row; current is `select`; a more button appears on hover or focus.

### Inputs
36u high, 6u radius, `line-strong` border on panel white; focus changes the border to ink (no glow). Inline title fields are borderless and gain chrome with an inset `line-strong` ring on hover and focus. Checkboxes use ink as the accent. Global focus-visible: a 2u ink outline at 2u offset.

### Menus (`native/ui/widgets.slint` and native PopupWindow surfaces)
Panel white, 8u radius, hairline border, floating shadow, 6u padding; 36u items at 15u with 17u `ink-3` icons; highlighted chrome; danger items in `danger`.

### Speakers tab
Sections separated by hairlines, each with a 16u 600 title; speaker rows show a tone dot, name, line count in `ink-3` and a trailing tool; rename and merge are inline forms built from inputs and small buttons.

### First run (`frontend/src/components/onboarding/steps/WelcomeStep.tsx`)
Centred: the mark and name, a headline, a lede in `ink-2`, a miniature session (three speaker lanes and a decision clip under a red playhead) and an ink "Get started" button.

### Motion
One easing, `--ot-ease: cubic-bezier(0.16, 1, 0.3, 1)`; state changes 140 to 160 ms. Only a chosen moment moves:
- Choosing a clip glides the playhead 400 ms to its moment (`data-gliding`).
- The newly current transcript line arrives with a 520 ms flash from `select-strong` with an ink left edge.
- A pressed clip bobs 2u over 320 ms.
- The inspector quote focuses in 360 ms from 0.35 opacity and a 2 px blur.
- The live dot breathes on a 1.6 s loop; the drawer rail slides 240 ms; the first-run playhead sweeps a 6 s alternate loop.
All looping and decorative motion sits behind `prefers-reduced-motion: no-preference`, and reduced motion zeroes transitions across the app and menus.

### Setup, tour and guide (`frontend/src/components/onboarding/`, `frontend/src/components/guide/`)
- **Setup** asks one question per screen under a labelled step bar: Welcome, Speech model, Permissions (macOS only), Your sound. The sound check shows a 24-bar microphone meter (teal, amber in the top four bars) and saves the chosen devices. Finishing sets `overtalk.tour.pending` so the first launch opens with the tour.
- **Tour** spotlights one real control at a time (`data-tour` targets) with a 2u ink ring and a 50% scrim; the card has Skip, a count, Back and one ink Next. Missing targets are skipped. The home tour runs once after setup; the review tour runs once on the first meeting with clips, never while the guide is open.
- **Guide** is the manual: a 600u drawer docked right that never blocks the session, also a page at `/guide`. Chapters are grouped Start, Record, Review, Notes, Find, Privacy and settings; each shows a real screenshot (`public/guide/*.webp`, captured from the app) with numbered markers whose positions come from `markers.json`; the active step gets an ink ring and dims the rest. Read progress is per device. F1 or `?` opens it. `tests/lib/guide.test.ts` fails when a step points at a control missing from its screenshot.

### Brand mark (`frontend/public/brand/icon.svg`, imported by native Slint)
Two curved speech shapes in teal and magenta. Variation 17b is the chosen mark; keep its SVG geometry and colors. `native/assets/brand-light.svg` and `native/assets/brand-dark.svg` carry the shell mark; `frontend/public/brand/icon.svg` supplies the app icon.

## Do's and Don'ts

### Do:
- **Do** use native `Theme.scale` and layout constraints; the retained web reference uses `calc(n * var(--u))` against its 1536 px frame.
- **Do** fill primary actions with ink and keep one primary per group.
- **Do** assign speaker tones in first-appearance order (teal, magenta, slate, moss, clay, plum) through `.ot-tone-*` classes.
- **Do** keep amber for decisions and violet for actions, as lanes and clips on the same timeline as speech.
- **Do** set every time value in tabular numerals, with `timecode()` in the transport and `clock()` elsewhere.
- **Do** separate surfaces with 1 px hairlines and tonal steps; reserve shadows for panels and floating layers.
- **Do** honor native `State.reduced-motion`; the retained web frontend uses `prefers-reduced-motion: no-preference` and `--ot-ease`.
- **Do** write controls and headings in sentence case ("Copy transcript", "Play from 03:09", "Library & backups"), plain and specific.
- **Do** keep native window decorations.
- **Do** keep speaker labels provisional in wording; nothing claims an identity the app has not confirmed.

### Don't:
- **Don't** use blue anywhere: no blue buttons, links, focus rings, selections or accents.
- **Don't** use red for anything except recording and the playhead.
- **Don't** give a speaker amber or violet, or a note a speaker tone.
- **Don't** use Phudu outside the transport timecode.
- **Don't** add kicker or eyebrow labels above headings, or uppercase tracked micro-labels.
- **Don't** title-case or uppercase controls.
- **Don't** add drop shadows to lanes, rows or clips at rest.
- **Don't** redraw the chosen 17b mark without a deliberate brand change.
- **Don't** replace the native title bar with custom window chrome.
