#!/bin/bash
# Download the AMI test-set audio and manual annotations (CC BY 4.0) for this evaluation.
# Usage: evaluations/ami/fetch.sh [directory]   (default ~/.cache/overtalk-eval/ami)
# Existing files are kept, so a failed run can simply be repeated. Nothing here is committed.
set -u
directory="${1:-$HOME/.cache/overtalk-eval/ami}"
mkdir -p "$directory/annotations" "$directory/audio"
annotations="$directory/annotations/ami_public_manual_1.6.2.zip"
if [ ! -s "$annotations" ]; then
  curl -sSfL --retry 3 -o "$annotations.part" \
    https://groups.inf.ed.ac.uk/ami/AMICorpusAnnotations/ami_public_manual_1.6.2.zip \
    && mv -f "$annotations.part" "$annotations" && echo "annotations ok" || echo "annotations FAILED"
fi
for meeting in EN2002a EN2002b EN2002c EN2002d ES2004a ES2004b ES2004c ES2004d \
               IS1009a IS1009b IS1009c IS1009d TS3003a TS3003b TS3003c TS3003d; do
  for kind in Mix-Headset Array1-01; do
    file="$directory/audio/$meeting.$kind.wav"
    [ -s "$file" ] && continue
    curl -sSfL --retry 3 -o "$file.part" \
      "https://groups.inf.ed.ac.uk/ami/AMICorpusMirror//amicorpus/$meeting/audio/$meeting.$kind.wav" \
      && mv -f "$file.part" "$file" && echo "$meeting.$kind ok" || echo "$meeting.$kind FAILED"
  done
done
