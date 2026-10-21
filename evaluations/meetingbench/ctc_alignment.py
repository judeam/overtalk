"""Compatibility import for the alignment evaluation harness."""
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "speech-worker"))
from sonefold_speech.ctc import TokenSpan, align_tokens, sequence_log_probability  # noqa: E402,F401
