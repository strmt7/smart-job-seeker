"""Offline adversarial checks for the runtime repair worker; no host installation."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import types
import zipfile
import unittest
from unittest.mock import patch


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(file))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


worker = load('runtime_repair', 'runtime-setup.py')
package = load('runtime_package', 'package-windows.py')


class RepairTests(unittest.TestCase):
    def test_real_ollama_archive_checks_and_vcrt_inventory_share_sink(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            entries = [('base.zip', ['ollama.exe', 'lib/ollama/llama_cpp_license', 'lib/ollama/vulkan/ggml-vulkan.dll']),
                       ('gpu.zip', ['lib/ollama/rocm_v7_1/ggml-hip.dll'])]
            specs = []
            for name, files in entries:
                archive = root / name
                with zipfile.ZipFile(archive, 'w') as output:
                    for file in files:
                        output.writestr(file, b'fictional bytes')
                specs.append((name, 'https://example.invalid/'+name, archive.stat().st_size, worker.digest(archive), 4096))
            vcrt = root / 'vcrt'
            vcrt.mkdir()
            (vcrt / 'vcruntime140.dll').write_bytes(b'fictional dll')
            spec = dict(package.RUNTIME_CATALOG['msvc'])
            spec['files'] = {'vcruntime140.dll': worker.digest(vcrt / 'vcruntime140.dll')}
            original = package.add_ollama_runtime
            with patch.dict(package.RUNTIME_CATALOG, msvc=spec), \
                 patch.object(package, 'add_ollama_runtime', side_effect=lambda sink: original(sink, local_archives=root, specs=specs)), \
                 patch.object(package, 'provision_vcrt', return_value=vcrt), \
                 patch.object(worker, 'probe', return_value='Fictional Ollama 1'):
                result = worker.install(package, 'ollama', root)
            self.assertEqual(result['tool'], 'ollama')
            manifest = json.loads((root / 'output/inventory.json').read_text())
            worker.verify_inventory(root / 'output', manifest['files'])
            self.assertTrue((root / 'output/app/dependencies/ollama/LICENSE').is_file())

    def test_output_sink_rejects_escape_windows_aliases_and_case_duplicates(self):
        with tempfile.TemporaryDirectory() as directory:
            sink = worker.DirectoryArchive(Path(directory), package.check_windows_bundle_path)
            for name in ['../bad', '/app/x', 'app/../bad', 'app/C:/bad', 'app/CON.txt',
                         'app/bad.', 'app/bad ', 'app/a\\b', 'app/a\x00b']:
                with self.subTest(name=name), self.assertRaises(ValueError):
                    sink.writestr(name, b'bad')
            sink.writestr('app/one', b'first')
            with self.assertRaises(ValueError):
                sink.writestr('app/ONE', b'replacement')
            self.assertEqual((Path(directory) / 'app/one').read_bytes(), b'first')

    def test_readback_detects_corruption_omissions_and_extra_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            sink = worker.DirectoryArchive(root, package.check_windows_bundle_path)
            sink.writestr('app/file', b'good')
            hashes = {'file': hashlib.sha256(b'good').hexdigest()}
            worker.verify_inventory(root, hashes)
            (root / 'app/file').write_bytes(b'bad')
            with self.assertRaises(ValueError):
                worker.verify_inventory(root, hashes)
            (root / 'app/file').write_bytes(b'good')
            (root / 'app/extra').write_bytes(b'extra')
            with self.assertRaises(ValueError):
                worker.verify_inventory(root, hashes)
            with self.assertRaises(ValueError):
                worker.verify_inventory(root, {})

    def test_unknown_components_and_low_disk_do_not_create_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(ValueError):
                worker.install(package, '../browser', root)
            with patch.object(worker.shutil, 'disk_usage', return_value=types.SimpleNamespace(free=0)):
                with self.assertRaises(ValueError):
                    worker.install(package, 'browser', root)
            self.assertFalse((root / 'output').exists())

    def test_real_packager_browser_inventory_streams_to_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            engine = root / 'engine'
            engine.mkdir()
            for name in ['chrome-headless-shell.exe', 'LICENSE.headless_shell', 'icudtl.dat']:
                (engine / name).write_bytes(('fictional ' + name).encode())
            with patch.object(package, 'provision_chromium', return_value=engine), \
                 patch.object(worker, 'probe', return_value='Fictional engine 1'):
                result = worker.install(package, 'browser', root)
            self.assertEqual(result['files'], 3)
            manifest = json.loads((root / 'output/inventory.json').read_text())
            self.assertEqual(manifest['tool'], 'browser')
            worker.verify_inventory(root / 'output', manifest['files'])
            with patch.object(package, 'provision_chromium', return_value=engine):
                with self.assertRaises(FileExistsError):
                    worker.install(package, 'browser', root)

    def test_failed_probe_never_writes_activation_inventory(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            engine = root / 'engine'
            engine.mkdir()
            for name in ['chrome-headless-shell.exe', 'LICENSE.headless_shell']:
                (engine / name).write_bytes(b'fictional')
            with patch.object(package, 'provision_chromium', return_value=engine), \
                 patch.object(worker, 'probe', side_effect=ValueError('failed probe')):
                with self.assertRaisesRegex(ValueError, 'failed probe'):
                    worker.install(package, 'browser', root)
            self.assertFalse((root / 'output/inventory.json').exists())

    def test_vcrt_and_office_notices_use_the_same_directory_sink(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            office, vcrt = root / 'office', root / 'vcrt'
            (office / 'program').mkdir(parents=True)
            vcrt.mkdir()
            (office / 'program/soffice.com').write_bytes(b'fictional converter')
            (office / 'LICENSE.html').write_bytes(b'fictional license')
            spec = dict(package.RUNTIME_CATALOG['msvc'])
            spec['files'] = {'vcruntime140.dll': hashlib.sha256(b'fictional dll').hexdigest()}
            (vcrt / 'vcruntime140.dll').write_bytes(b'fictional dll')
            with patch.dict(package.RUNTIME_CATALOG, msvc=spec), \
                 patch.object(package, 'provision_libreoffice', return_value=office), \
                 patch.object(package, 'provision_vcrt', return_value=vcrt), \
                 patch.object(worker, 'probe', return_value='Fictional office 1'):
                result = worker.install(package, 'office', root)
            self.assertEqual(result['tool'], 'office')
            self.assertTrue((root / 'output/app/licenses/MSVC_RUNTIME_LICENSE.docx').is_file())
            self.assertEqual((root / 'output/app/dependencies/libreoffice/program/vcruntime140.dll').read_bytes(), b'fictional dll')


if __name__ == '__main__':
    unittest.main()
