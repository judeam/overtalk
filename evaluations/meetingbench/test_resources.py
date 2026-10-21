import sys
import types
import unittest
from unittest.mock import patch

from .resources import peak_rss_kib


class ResourceTests(unittest.TestCase):
    def test_unsupported_platform_reports_unknown(self):
        with patch.dict(sys.modules, {'resource': None}):
            self.assertIsNone(peak_rss_kib())

    def test_macos_byte_measurements_are_converted_to_kib(self):
        fake = types.SimpleNamespace(RUSAGE_SELF=0, getrusage=lambda _: types.SimpleNamespace(ru_maxrss=4096))
        with patch.dict(sys.modules, {'resource': fake}), patch.object(sys, 'platform', 'darwin'):
            self.assertEqual(peak_rss_kib(), 4)
        with patch.dict(sys.modules, {'resource': fake}), patch.object(sys, 'platform', 'linux'):
            self.assertEqual(peak_rss_kib(), 4096)
