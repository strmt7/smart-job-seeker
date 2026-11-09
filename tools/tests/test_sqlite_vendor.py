import importlib.util
from pathlib import Path
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('sqlite_vendor', ROOT / 'tools/verify_sqlite_vendor.py')
vendor = importlib.util.module_from_spec(spec)
spec.loader.exec_module(vendor)


class SqliteVendorTests(unittest.TestCase):
    def test_reviewed_archive_and_exact_patch(self):
        self.assertEqual(vendor.verify()['upstream_files'], 22)

    def test_source_or_manifest_drift_is_rejected(self):
        read = Path.read_bytes
        for suffix in ['src/lib.rs', 'Cargo.toml']:
            with self.subTest(suffix=suffix):
                def altered(path):
                    data = read(path)
                    return data + b'\n/* fictional drift */\n' if path.as_posix().endswith(suffix) else data
                with mock.patch.object(Path, 'read_bytes', altered):
                    with self.assertRaisesRegex(ValueError, 'bytes changed'):
                        vendor.verify()
