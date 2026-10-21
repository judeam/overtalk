"""Optional process memory observations without a platform-only import."""
import sys


def peak_rss_kib():
    try:
        import resource
    except ImportError:
        return None
    value = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    return int(value / 1024) if sys.platform == 'darwin' else int(value)
