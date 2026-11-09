"""Check cross-host inventory stability and explicit license/graph failures."""
import copy
import importlib.util
from pathlib import Path
import unittest

path = Path(__file__).resolve().parents[1] / 'scripts' / 'update-license-inventory.py'
spec = importlib.util.spec_from_file_location('license_inventory', path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class InventoryChecks(unittest.TestCase):
    def setUp(self):
        self.metadata = {
            'packages': [{'id': 'fixture', 'name': 'fictional', 'version': '1.0.0',
                          'license': '(MIT OR Apache-2.0) AND Unicode-3.0'}],
            'resolve': {'nodes': [{'id': 'fixture'}]},
            'workspace_members': ['fixture'],
        }

    def test_windows_and_unix_checkouts_have_identical_inventories(self):
        unix = module.inventory(self.metadata, b'version = 4\n\n')
        windows = module.inventory(self.metadata, b'version = 4\r\n\r\n')
        self.assertEqual(unix, windows)
        self.assertIn('(MIT OR Apache-2.0) AND Unicode-3.0 (1): fictional@1.0.0', unix)
        self.assertIn('1 total; 1 workspace; 0 external', unix)

    def test_missing_or_multiline_license_never_becomes_a_clean_inventory(self):
        for license_value in [None, '', 'MIT\nUnknown']:
            with self.subTest(license=license_value):
                self.metadata['packages'][0]['license'] = license_value
                with self.assertRaises(ValueError):
                    module.inventory(self.metadata, b'lock')

    def test_incomplete_and_duplicated_package_metadata_is_refused(self):
        duplicate = copy.deepcopy(self.metadata)
        duplicate['packages'].append(copy.deepcopy(duplicate['packages'][0]))
        missing = copy.deepcopy(self.metadata)
        missing['resolve']['nodes'].append({'id': 'missing'})
        for metadata in [duplicate, missing]:
            with self.assertRaises(ValueError):
                module.inventory(metadata, b'lock')


if __name__ == '__main__':
    unittest.main()
