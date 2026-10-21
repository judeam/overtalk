"""Optional, offline speech analysis for Sonefold's native desktop app."""

import os

# Set before importing model libraries. Analysis never fetches model files or audio.
os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"
os.environ["HF_HUB_DISABLE_TELEMETRY"] = "1"
os.environ["PYANNOTE_METRICS_ENABLED"] = "0"

__version__ = "0.1.0"
