// The Overtalk guide: short chapters, each a real screen with numbered steps.
// `target` names a `data-tour` element; the capture script records where it sits in each screenshot.

export type GuideAction =
  | { kind: 'tour'; tour: TourId; label: string }
  | { kind: 'go'; href: string; label: string }
  | { kind: 'event'; event: string; label: string };

interface GuideStep { title: string; body: string; target?: string }

export interface GuideLesson {
  id: string;
  group: 'Start' | 'Record' | 'Review' | 'Notes' | 'Find' | 'Privacy';
  title: string;
  summary: string;
  /** Screenshot under /guide/, captured from the app itself. */
  shot: GuideShot;
  steps: GuideStep[];
  tips?: string[];
  actions?: GuideAction[];
  keys?: string[];
}

type GuideShot = 'home' | 'live' | 'review' | 'speakers' | 'notes' | 'search' | 'library' | 'settings';

export type TourId = 'home' | 'review';

export const GROUPS: GuideLesson['group'][] = ['Start', 'Record', 'Review', 'Notes', 'Find', 'Privacy'];

export const LESSONS: GuideLesson[] = [
  {
    id: 'sound',
    group: 'Start',
    title: 'Set up your sound',
    summary: 'Choose the microphone and the call audio Overtalk listens to.',
    shot: 'home',
    steps: [
      { title: 'Pick your microphone', body: 'Select Mic at the top right. Your choice is remembered for every recording.', target: 'mic' },
      { title: 'Pick the call audio', body: 'Select System to choose what the call plays through, so the other people are recorded too.', target: 'system' },
      { title: 'Check the levels', body: 'Settings, Recordings shows a live meter: speak and watch it move before an important call.', target: 'settings' },
    ],
    tips: ['Wearing headphones keeps your microphone from picking up the call twice.'],
    actions: [{ kind: 'go', href: '/settings?tab=recording', label: 'Open sound settings' }],
  },
  {
    id: 'tour',
    group: 'Start',
    title: 'Know your way around',
    summary: 'One screen holds everything: your meetings, the timeline, the transcript and the selected moment.',
    shot: 'review',
    steps: [
      { title: 'Meetings', body: 'Recordings and imports land here. Search, pin, archive and sort your meetings; waveform previews are optional.', target: 'rail' },
      { title: 'Transport', body: 'Record, play and stop, the clock, and the meeting title. Click the title to rename it.', target: 'transport' },
      { title: 'Lanes', body: 'The timeline opens with the meeting. Auto fit shows its lanes using the available height; hide it when you want more room to read.', target: 'lanes' },
      { title: 'Transcript', body: 'Click a timestamp to listen, select text to copy it, or choose Edit to correct it.', target: 'transcript' },
      { title: 'Inspector', body: 'Whatever you select appears here, with Play from and Copy.', target: 'inspector' },
    ],
    actions: [{ kind: 'tour', tour: 'review', label: 'Take the tour on this screen' }],
  },
  {
    id: 'record',
    group: 'Record',
    title: 'Record a meeting',
    summary: 'Press record before the call starts; Overtalk transcribes as people speak.',
    shot: 'live',
    steps: [
      { title: 'Press record', body: 'The red button starts recording your microphone and the call audio.', target: 'record' },
      { title: 'Watch the clock', body: 'The clock runs while you record. Pause keeps everything so far; stop saves the meeting.', target: 'timecode' },
      { title: 'Name it', body: 'Type a title now or later. Untitled recordings are named by date and time.', target: 'title' },
      { title: 'Voices get lanes', body: 'The speaker timeline opens during recording and shows the voices detected so far. These labels are provisional.', target: 'lanes' },
    ],
    tips: ['Recording keeps going if you open another meeting. Press the red button to come back.'],
    keys: ['Space plays or pauses a saved meeting'],
    actions: [{ kind: 'go', href: '/', label: 'Go to the recorder' }],
  },
  {
    id: 'marks',
    group: 'Record',
    title: 'Mark moments while you talk',
    summary: 'Press M when something matters. The mark keeps the second and the line said then.',
    shot: 'live',
    steps: [
      { title: 'Press M or the button', body: 'Mark this moment works only while recording. You never lose your place in the call.', target: 'mark' },
      { title: 'Name it if you like', body: 'Click the mark to give it a name, such as “Budget agreed”. Empty names show who was speaking.', target: 'marks' },
      { title: 'Find it later', body: 'Marks appear as pins on the timeline of the saved meeting. Click one to play from it.', target: 'ruler' },
    ],
    keys: ['M marks the current moment while recording'],
  },
  {
    id: 'read',
    group: 'Review',
    title: 'Read a meeting at a glance',
    summary: 'See who spoke when, where people talked over each other, and what was decided.',
    shot: 'review',
    steps: [
      { title: 'Scrub the ruler', body: 'Drag along the time ruler to move the red playhead. Arrow keys move it by a second, Shift by ten.', target: 'ruler' },
      { title: 'Read the clips', body: 'Each clip is one stretch of speech, drawn from the recording. Overlapping clips are people talking at once.', target: 'lanes' },
      { title: 'Follow the transcript', body: 'Enable Follow playback to keep the current line visible. Scrolling pauses following until you enable it again.', target: 'transcript' },
    ],
    keys: ['Space plays or pauses', '← → move the playhead when the ruler is focused'],
  },
  {
    id: 'decisions',
    group: 'Review',
    title: 'Jump to a decision',
    summary: 'Decisions and actions from your notes are placed at the moment they were said.',
    shot: 'review',
    steps: [
      { title: 'Click a decision', body: 'Amber clips are decisions, violet clips are actions. Click one and the playhead moves to that second.', target: 'decisions' },
      { title: 'Read it in full', body: 'The inspector separates the generated interpretation from the original transcript evidence.', target: 'inspector' },
      { title: 'Hear it', body: 'Play from starts the recording at that moment, so you can check the exact words.', target: 'play-from' },
    ],
    tips: ['A decision only gets a clip when the notes point to a line of the transcript. Regenerate notes after editing the transcript.'],
  },
  {
    id: 'solo',
    group: 'Review',
    title: 'Listen to one person',
    summary: 'Play only one speaker, or skip someone, without editing anything.',
    shot: 'review',
    steps: [
      { title: 'Solo', body: 'Only plays that speaker and skips the silence between their turns.', target: 'keys' },
      { title: 'Mute', body: 'Skip skips that speaker unless someone else is talking over them.', target: 'keys' },
    ],
  },
  {
    id: 'speakers',
    group: 'Review',
    title: 'Name the speakers',
    summary: 'Give voices real names, merge duplicates, or analyse the recording again.',
    shot: 'speakers',
    steps: [
      { title: 'Open Speakers', body: 'The Speakers tab sits next to Transcript.', target: 'speakers-tab' },
      { title: 'Rename or merge', body: 'Rename a speaker for this meeting, or merge two labels that are the same person.', target: 'speaker-list' },
      { title: 'Improve the transcript', body: 'Open Meeting details for Analyse recording. It saves a recovery copy before transcribing again.' },
    ],
  },
  {
    id: 'notes',
    group: 'Notes',
    title: 'Write notes',
    summary: 'Turn the transcript into notes with decisions and actions you can edit.',
    shot: 'notes',
    steps: [
      { title: 'Open the notes', body: 'The Notes tab opens the notes for this meeting.', target: 'notes-button' },
      { title: 'Generate', body: 'Generate notes writes them from the transcript. Template and Model change what is written and by which model.', target: 'generate' },
      { title: 'Edit freely', body: 'Your edits save automatically. A local draft protects changes while saving; Copy notes puts them on the clipboard.', target: 'notes-body' },
    ],
    tips: ['Expand Context for your notes in the Notes tab to add names, project words or what to focus on before generating.'],
  },
  {
    id: 'export',
    group: 'Notes',
    title: 'Export and share',
    summary: 'Save notes and transcript as Markdown, Word, PDF, text or JSON.',
    shot: 'notes',
    steps: [
      { title: 'Export', body: 'Choose the notes, the transcript or both, then the format.', target: 'export' },
      { title: 'Copy a moment', body: 'In the inspector, Copy takes the quote; the menu adds its time and meeting.', target: 'notes-body' },
    ],
  },
  {
    id: 'search',
    group: 'Find',
    title: 'Search your meetings',
    summary: 'Find a meeting by its title or by anything said in it.',
    shot: 'search',
    steps: [
      { title: 'Open search', body: 'Press Ctrl+K to focus Search all meetings above the meeting list.', target: 'search' },
      { title: 'Type a word', body: 'Matching meetings show transcript passages underneath. Select a passage to jump directly to its source.', target: 'rail' },
    ],
    keys: ['Ctrl+K opens search', 'Esc closes it'],
  },
  {
    id: 'ask',
    group: 'Find',
    title: 'Ask across meetings',
    summary: 'Ask a question and get an answer that quotes the lines it came from.',
    shot: 'library',
    steps: [
      { title: 'Open the library', body: 'Choose Library above the meeting list.', target: 'library-button' },
      { title: 'Build the index once', body: 'Exact text search works immediately. Library settings can add a local semantic index for searching by meaning.', target: 'library-settings' },
      { title: 'Search or ask', body: 'Search finds passages; Ask writes an answer with citations you can open.', target: 'library-search' },
    ],
    actions: [{ kind: 'go', href: '/library', label: 'Open the library' }],
  },
  {
    id: 'privacy',
    group: 'Privacy',
    title: 'What stays on this computer',
    summary: 'Recordings, transcripts and notes are stored locally. Cloud AI is used only when you choose a cloud provider.',
    shot: 'settings',
    steps: [
      { title: 'Local by default', body: 'Speech recognition and the default notes model run on this computer.', target: 'settings-nav' },
      { title: 'Your choice of model', body: 'In Summary you can choose a cloud provider instead; meeting text is then sent to it.', target: 'settings-nav' },
      { title: 'Where files live', body: 'General shows the recordings folder. Move meetings to Trash from their menu, then restore or permanently delete them in Trash.', target: 'settings-body' },
    ],
    actions: [{ kind: 'go', href: '/settings', label: 'Open settings' }],
  },
];

export const SHORTCUTS: [string, string][] = [
  ['Space', 'Play or pause the meeting'],
  ['M', 'Mark this moment while recording'],
  ['Ctrl+K', 'Search meetings'],
  ['← →', 'Move the playhead (ruler focused); Shift moves ten seconds'],
  ['F1 or ?', 'Open this guide'],
  ['Esc', 'Close search, menus, the tour or this guide'],
];

export interface TourStep { target: string; title: string; body: string }

export const TOURS: Record<TourId, TourStep[]> = {
  home: [
    { target: 'record', title: 'Record', body: 'Start recording your microphone and the call. The clock runs while you record.' },
    { target: 'sources', title: 'Your sound', body: 'Choose the microphone and the call audio. Overtalk remembers them.' },
    { target: 'sources', title: 'Check your sound', body: 'Record a short sound check before an important call, then listen back.' },
    { target: 'mark', title: 'Mark moments', body: 'Press M during a call when something matters. Marks stay with the meeting.' },
    { target: 'rail', title: 'Your meetings', body: 'Saved meetings appear here. Open one to read, play and search it.' },
    { target: 'guide', title: 'Help is here', body: 'The guide explains every part of Overtalk. Press F1 any time.' },
  ],
  review: [
    { target: 'transport', title: 'Playback', body: 'Skip ten seconds, change playback speed, or export this meeting.' },
    { target: 'transcript', title: 'Read and correct', body: 'Click a timestamp to listen. Select text to copy it; Edit corrects words and speakers.' },
    { target: 'inspector', title: 'The selected moment', body: 'Read it in full, play from it or copy it.' },
    { target: 'transcript', title: 'Transcript', body: 'Enable Follow playback to keep the current line in view. Search finds passages across the full transcript.' },
    { target: 'notes-button', title: 'Notes', body: 'Generate, edit and export notes for this meeting.' },
  ],
};
