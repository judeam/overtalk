"""Build whole-meeting AMI references from the NXT manual word annotations.

The archive is untrusted input: run this as `python3 -I`. Members are read by
their expected names straight from the zip (nothing is extracted), sizes are
bounded, and documents that declare a DTD or entities are rejected.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import xml.etree.ElementTree as ET
import zipfile

# The standard AMI full-corpus ASR test partition.
TEST_MEETINGS = tuple(f"{prefix}{part}" for prefix in ("EN2002", "ES2004", "IS1009", "TS3003")
                      for part in "abcd")
NITE = "{http://nite.sourceforge.net/}"
MAX_MEMBER_BYTES = 16 * 1024 * 1024
HREF = re.compile(r"^(?P<file>[^#]+)#id\((?P<first>[^)]+)\)(?:\.\.id\((?P<last>[^)]+)\))?$")
# AMI spells letter-by-letter acronyms as "T_V_", "L_C_D_s", "anti-R_S_I_".
ACRONYM_LETTER = re.compile(r"(?<=[A-Za-z])_")


def ami_orthography(word: str) -> str:
    """Write AMI letter-by-letter acronyms the way a transcript does ("T_V_" -> "TV")."""
    return ACRONYM_LETTER.sub("", word)


def parse_xml(data: bytes) -> ET.Element:
    if b"<!DOCTYPE" in data or b"<!ENTITY" in data:
        raise ValueError("Annotation declares a DTD or entities; refusing to parse")
    return ET.fromstring(data)


def read_member(archive: zipfile.ZipFile, name: str) -> bytes | None:
    try:
        info = archive.getinfo(name)
    except KeyError:
        return None
    if info.file_size > MAX_MEMBER_BYTES:
        raise ValueError(f"{name} is larger than expected")
    with archive.open(info) as member:
        data = member.read(MAX_MEMBER_BYTES + 1)
    if len(data) > MAX_MEMBER_BYTES:
        raise ValueError(f"{name} is larger than expected")
    return data


def parse_words(data: bytes):
    """Return (element ids in document order, {id: word}) for one speaker's words file."""
    order, words = [], {}
    for element in parse_xml(data):
        identifier = element.get(NITE + "id")
        if identifier is None:
            continue
        order.append(identifier)
        if element.tag != "w" or element.get("punc") == "true":
            continue
        text = (element.text or "").strip()
        if not text:
            continue
        start, end = element.get("starttime"), element.get("endtime")
        words[identifier] = {
            "text": ami_orthography(text),
            "start": float(start) if start is not None else None,
            "end": float(end) if end is not None else None,
            "truncated": element.get("trunc") == "true",
        }
    return order, words


def parse_segments(data: bytes, words_file: str):
    """Yield (start, end, first id, last id) for each transcribed segment."""
    for segment in parse_xml(data):
        if segment.tag != "segment":
            continue
        start = float(segment.get("transcriber_start"))
        end = float(segment.get("transcriber_end"))
        for child in segment:
            if child.tag != NITE + "child":
                continue
            match = HREF.match(child.get("href", ""))
            if not match or match["file"] != words_file:
                raise ValueError(f"Unexpected segment reference in {words_file}")
            yield start, end, match["first"], match["last"] or match["first"]


def overlap_fraction(start: float, end: float, spans) -> float:
    """Share of [start, end] during which any of `spans` (other participants' words) is spoken."""
    pieces = sorted((max(s, start), min(e, end)) for s, e in spans if s < end and e > start)
    covered, cursor = 0.0, start
    for piece_start, piece_end in pieces:
        piece_start = max(piece_start, cursor)
        if piece_end > piece_start:
            covered += piece_end - piece_start
            cursor = piece_end
    return covered / (end - start) if end > start else 0.0


def speaker_utterances(words_data: bytes, segments_data: bytes | None, words_file: str, speaker: str):
    """Return this participant's utterances and the (start, end) time of each spoken word."""
    order, words = parse_words(words_data)
    position = {identifier: index for index, identifier in enumerate(order)}
    covered, utterances = set(), []
    for start, end, first, last in (parse_segments(segments_data, words_file) if segments_data else ()):
        ids = order[position[first]: position[last] + 1]
        tokens = [words[identifier] for identifier in ids if identifier in words]
        covered.update(ids)
        if tokens:
            utterances.append({"speaker": speaker, "start": start, "end": end,
                               "words": [token["text"] for token in tokens],
                               "truncated": [token["truncated"] for token in tokens]})
    # A word outside every segment still belongs to the meeting; keep it at its own time.
    for identifier in order:
        if identifier in words and identifier not in covered:
            token = words[identifier]
            utterances.append({"speaker": speaker, "start": token["start"], "end": token["end"],
                               "words": [token["text"]], "truncated": [token["truncated"]],
                               "outside_segments": True})
    spans = [(token["start"], token["end"]) for token in words.values()
             if token["start"] is not None and token["end"] is not None]
    return utterances, spans


def meeting_reference(archive: zipfile.ZipFile, meeting: str) -> dict:
    utterances, speakers, spans = [], [], {}
    for speaker in "ABCDEFGH":
        words_file = f"{meeting}.{speaker}.words.xml"
        words_data = read_member(archive, f"words/{words_file}")
        if words_data is None:
            continue
        speakers.append(speaker)
        segments_data = read_member(archive, f"segments/{meeting}.{speaker}.segments.xml")
        own, spans[speaker] = speaker_utterances(words_data, segments_data, words_file, speaker)
        utterances.extend(own)
    if not utterances:
        raise ValueError(f"No words found for {meeting}")
    for row in utterances:
        others = [span for speaker, rows in spans.items() if speaker != row["speaker"] for span in rows]
        row["overlap_fraction"] = round(overlap_fraction(row["start"], row["end"], others), 4)
    # Utterances in start-time order; words keep their order inside an utterance.
    utterances.sort(key=lambda row: (row["start"], row["speaker"], row["end"]))
    return {"meeting": meeting, "speakers": speakers, "utterances": utterances,
            "reference_words": sum(len(row["words"]) for row in utterances),
            "truncated_words": sum(sum(row["truncated"]) for row in utterances),
            "words_outside_segments": sum(1 for row in utterances if row.get("outside_segments"))}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True, help="ami_public_manual_1.6.2.zip")
    parser.add_argument("--output", type=Path, required=True, help="Directory for <meeting>.json references")
    parser.add_argument("--meetings", nargs="*", default=list(TEST_MEETINGS))
    args = parser.parse_args()
    digest = hashlib.sha256(args.archive.read_bytes()).hexdigest()
    args.output.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(args.archive) as archive:
        for meeting in args.meetings:
            reference = meeting_reference(archive, meeting)
            reference["archive"] = args.archive.name
            reference["archive_sha256"] = digest
            (args.output / f"{meeting}.json").write_text(json.dumps(reference, ensure_ascii=False))
            print(f"{meeting}: {reference['reference_words']} words, {len(reference['utterances'])} utterances, "
                  f"{reference['truncated_words']} truncated, {reference['words_outside_segments']} outside segments")


if __name__ == "__main__":
    main()
