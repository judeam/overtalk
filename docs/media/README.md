# README media

These captures use the real Slint UI and Rust controller with the synthetic
[`visual.json`](../../native/tests/fixtures/visual.json) service fixture. They
contain invented meetings and people, with no personal data, credentials or
production database access. They demonstrate the interface, not ASR or summary
accuracy. Screenshots are not AI-generated or composited product mockups.

- `notes.png`: editable notes, compact timeline and source-linked decisions.
- `transcript.png`: named speakers, transcript and audio lanes.
- `library.png`: cross-meeting passage search.
- `workflow.gif`: 48 frames at 10 fps, playing from 01:12 in the transcript,
  opening Notes after two seconds, then pausing. It plays once, without sound.

## Reproduce

From the repository root, after the native build prerequisites are prepared:

```sh
cargo build --release -p overtalk-native --example visual_review
python3 scripts/capture-native-parity.py \
  --binary target/release/examples/visual_review \
  --output /tmp/overtalk-readme-stills \
  --sizes 1600x1000 --views notes,meeting,library
```

Copy the three captures to their names above. To capture motion, use a disposable
profile; keep all four directory overrides so this cannot use the real library:

```sh
OVERTALK_SERVICE="$PWD/native/tests/fixtures/engine.py" \
OVERTALK_FIXTURE="$PWD/native/tests/fixtures/visual.json" \
XDG_DATA_HOME=/tmp/overtalk-readme-demo/data \
XDG_CONFIG_HOME=/tmp/overtalk-readme-demo/config \
XDG_CACHE_HOME=/tmp/overtalk-readme-demo/cache \
OVERTALK_VISUAL_DEMO=1 \
  target/release/examples/visual_review \
  1600 1000 1 /tmp/overtalk-readme-demo/frames

ffmpeg -framerate 10 -i /tmp/overtalk-readme-demo/frames/frame-%03d.png \
  -filter_complex '[0:v]scale=1100:-2:flags=lanczos,split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=sierra2_4a:diff_mode=rectangle' \
  -loop -1 docs/media/workflow.gif
```

Use a fresh temporary directory when regenerating. Inspect the images at their
rendered README size, check the GIF duration and finite playback, and recheck
all relative links. Keep assets small without sacrificing readable text; do not
commit raw frames, screen recordings or environment directories.
