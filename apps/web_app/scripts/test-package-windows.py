"""Exercise optional converter bundling with fictional, small local files."""
import hashlib
import importlib.util
import io
import os
import sys
from pathlib import Path
import tempfile
import types
import zipfile
from unittest.mock import patch

script = Path(__file__).with_name('package-windows.py')
spec = importlib.util.spec_from_file_location('waypoint_packager', script)
packager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packager)

# Check Windows extraction rules even when the host filesystem cannot create
# reserved names or distinguish two files solely by case.
for component in ('LibreOffice', 'Chromium'):
    seen = set()
    packager.check_windows_bundle_path('program/naïve #1.dat', seen, component)
    for invalid in ('program/NAÏVE #1.DAT', 'program/CON.txt', 'program/file.', 'program/a:b'):
        try:
            packager.check_windows_bundle_path(invalid, seen, component)
        except ValueError:
            pass
        else:
            raise AssertionError(f'{component} accepted incompatible path {invalid}')

target = script.resolve().parents[3] / 'target'
target.mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='waypoint-license-fixture-', dir=target) as temp:
    root = Path(temp) / 'crate'
    (root / 'native').mkdir(parents=True)
    (root / 'LICENSE').write_text('fictional wrapper license', encoding='utf-8')
    (root / 'native' / 'LICENSE-component').write_text('fictional native license', encoding='utf-8')
    (root / 'NOTICE-native.txt').write_text('fictional native notice', encoding='utf-8')
    files = packager.dependency_license_files(root, {})
    assert {path.relative_to(root).as_posix() for path in files} == {
        'LICENSE', 'native/LICENSE-component', 'NOTICE-native.txt'
    }
    outside = Path(temp) / 'outside.txt'
    outside.write_text('fictional external material', encoding='utf-8')
    try:
        packager.dependency_license_files(root, {'license-file': '../outside.txt'})
    except ValueError:
        pass
    else:
        raise AssertionError('A dependency license escaped its source package')
with tempfile.TemporaryDirectory(prefix='waypoint-package-fixture-', dir=target) as temp:
    root = Path(temp) / 'office'
    (root / 'program').mkdir(parents=True)
    (root / 'LICENSE.html').write_text('fictional license', encoding='utf-8')
    (root / 'program' / 'soffice.com').write_bytes(b'fictional executable')
    (root / 'program' / 'naïve #1.dat').write_bytes(b'fictional data')
    program_files = Path(temp) / 'program-files'
    local_app = Path(temp) / 'local-app'
    (program_files / 'LibreOffice' / 'program').mkdir(parents=True)
    (program_files / 'LibreOffice' / 'program' / 'soffice.com').write_bytes(b'incomplete')
    user_office = local_app / 'Programs' / 'LibreOffice'
    (user_office / 'program').mkdir(parents=True)
    (user_office / 'program' / 'soffice.com').write_bytes(b'fictional executable')
    (user_office / 'LICENSE.txt').write_text('fictional license', encoding='utf-8')
    env = {'PROGRAMFILES': str(program_files), 'LOCALAPPDATA': str(local_app)}
    assert packager.discover_libreoffice(env) == user_office
    (program_files / 'LibreOffice' / 'LICENSE.html').write_text('fictional license', encoding='utf-8')
    assert packager.discover_libreoffice(env) == program_files / 'LibreOffice'
    assert packager.discover_libreoffice({}) is None
    assert packager.select_libreoffice(False, None, {}) is None  # requires provisioning
    assert packager.select_libreoffice(True, None, {}) is None
    assert packager.select_libreoffice(False, str(root), {}) == root
    assert packager.select_libreoffice(False, None, env) is None  # host packages never alter the default bundle
    msi = Path(temp) / 'fixture.msi'
    packager.copy_verified_office_msi(io.BytesIO(b'fixture'), msi, hashlib.sha256(b'fixture').hexdigest())
    assert msi.read_bytes() == b'fixture'
    try:
        packager.copy_verified_office_msi(io.BytesIO(b'wrong'), Path(temp) / 'bad.msi', hashlib.sha256(b'fixture').hexdigest())
    except ValueError as error:
        assert 'pinned SHA-256' in str(error)
    else:
        raise AssertionError('A mismatched installer was accepted')
    old_limit = packager.OFFICE_MSI_MAX_BYTES
    packager.OFFICE_MSI_MAX_BYTES = 2
    try:
        try:
            packager.copy_verified_office_msi(io.BytesIO(b'large'), Path(temp) / 'large.msi', hashlib.sha256(b'large').hexdigest())
        except ValueError as error:
            assert 'size limit' in str(error)
        else:
            raise AssertionError('An oversized installer was accepted')
    finally:
        packager.OFFICE_MSI_MAX_BYTES = old_limit
    if sys.platform == 'win32':
        stage = Path(temp) / 'bad-installer-stage'
        stage.mkdir()
        try:
            packager.provision_libreoffice(stage, msi)
        except ValueError as error:
            assert 'pinned SHA-256' in str(error)
        else:
            raise AssertionError('Provisioning accepted a mismatched installer')
        assert not (stage / 'waypoint-libreoffice.msi').exists()
        assert not (stage / 'office').exists()
        original_copy = packager.copy_verified_office_msi
        def copy_fixture(source, destination):
            return original_copy(source, destination, hashlib.sha256(b'fixture').hexdigest())

        stage = Path(temp) / 'successful-installer-stage'
        stage.mkdir()
        calls = []
        def fake_install(command, **kwargs):
            calls.append((command, kwargs))
            office = stage / 'office'
            (office / 'program').mkdir()
            (office / 'program' / 'soffice.com').write_bytes(b'fixture')
            (office / 'LICENSE.html').write_text('fixture', encoding='utf-8')
            (office / 'waypoint-libreoffice.msi').write_bytes(b'transformed')
            return types.SimpleNamespace(returncode=0)

        with patch.object(packager, 'copy_verified_office_msi', side_effect=copy_fixture), \
             patch.object(packager.shutil, 'disk_usage', return_value=types.SimpleNamespace(free=5 * 1024**3)), \
             patch.object(packager.subprocess, 'run', side_effect=fake_install):
            assert packager.provision_libreoffice(stage, msi) == stage / 'office'
        command, kwargs = calls[0]
        assert command[1:] == ['/a', str(stage / 'waypoint-libreoffice.msi'), '/qn', '/norestart', f'TARGETDIR={stage / "office"}']
        assert kwargs['timeout'] == 300
        assert not (stage / 'office' / 'waypoint-libreoffice.msi').exists()
        assert msi.read_bytes() == b'fixture'

        stage = Path(temp) / 'timed-out-installer-stage'
        stage.mkdir()
        with patch.object(packager, 'copy_verified_office_msi', side_effect=copy_fixture), \
             patch.object(packager.shutil, 'disk_usage', return_value=types.SimpleNamespace(free=5 * 1024**3)), \
             patch.object(packager.subprocess, 'run', side_effect=packager.subprocess.TimeoutExpired('msiexec', 300)):
            try:
                packager.provision_libreoffice(stage, msi)
            except ValueError as error:
                assert 'Windows Installer may still be active' in str(error)
            else:
                raise AssertionError('Timed-out extraction was accepted')
        assert (stage / 'waypoint-libreoffice.msi').read_bytes() == b'fixture'
        assert (stage / 'office').is_dir()
    configured = {'WAYPOINT_SOFFICE': str(root / 'program' / 'soffice.com'), **env}
    assert packager.discover_libreoffice(configured) == root
    path_env = {'PATH': os.pathsep.join([str(program_files / 'LibreOffice' / 'missing'), str(root / 'program')]), **env}
    assert packager.discover_libreoffice(path_env) == root
    invalid = {'WAYPOINT_SOFFICE': str(Path(temp) / 'missing' / 'program' / 'soffice.com'), **env}
    assert packager.discover_libreoffice(invalid) == program_files / 'LibreOffice'
    output = Path(temp) / 'bundle.zip'
    with zipfile.ZipFile(output, 'w') as archive:
        inventory = packager.add_libreoffice(archive, packager.discover_libreoffice(path_env))
    with zipfile.ZipFile(output) as archive:
        assert len(inventory) == 3
        for name, digest in inventory.items():
            assert hashlib.sha256(archive.read('app/' + name)).hexdigest() == digest
    (root / 'LICENSE.html').unlink()
    try:
        with zipfile.ZipFile(output, 'w') as archive:
            packager.add_libreoffice(archive, root)
    except ValueError as error:
        assert 'license' in str(error)
    else:
        raise AssertionError('A distribution without a license was accepted')

print('Fictional LibreOffice bundle and license checks passed.')

with tempfile.TemporaryDirectory(prefix='waypoint-chromium-fixture-', dir=target) as temp:
    root = Path(temp) / 'browser'
    root.mkdir()
    (root / 'chrome.exe').write_bytes(b'fictional browser')
    (root / 'LICENSES.chromium.html').write_text('fictional notices', encoding='utf-8')
    (root / 'resources.pak').write_bytes(b'fictional resources')
    output = Path(temp) / 'browser.zip'
    with zipfile.ZipFile(output, 'w') as archive:
        inventory = packager.add_chromium(archive, root)
    with zipfile.ZipFile(output) as archive:
        assert len(inventory) == 3
        for name, digest in inventory.items():
            assert hashlib.sha256(archive.read('app/' + name)).hexdigest() == digest
    (root / 'LICENSES.chromium.html').unlink()
    try:
        with zipfile.ZipFile(output, 'w') as archive:
            packager.add_chromium(archive, root)
    except ValueError as error:
        assert 'LICENSES.chromium.html' in str(error)
    else:
        raise AssertionError('A browser without notices was accepted')
print('Fictional Chromium bundle and notice checks passed.')

with tempfile.TemporaryDirectory(prefix='waypoint-browser-bootstrap-fixture-', dir=target) as temp:
    root = Path(temp)
    def browser_zip(entries):
        output = io.BytesIO()
        with zipfile.ZipFile(output, 'w') as source:
            for name, data in entries:
                source.writestr('engine/' + name, data)
        return output.getvalue()
    entries = [('chrome-headless-shell.exe', b'fixture'), ('icudtl.dat', b'fixture'), ('LICENSE.headless_shell', b'fictional notices')]
    for index, variant in enumerate([entries, entries + [('../outside', b'bad')], entries + [('ICUDTL.DAT', b'alias')]]):
        data = browser_zip(variant)
        package = root / f'engine-{index}.zip'
        package.write_bytes(data)
        stage = root / f'stage-{index}'
        stage.mkdir()
        spec = {'url': 'unused', 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest(), 'archive_prefix': 'engine/', 'max_unpacked_bytes': 1024}
        if index == 0:
            browser = packager.provision_chromium(stage, package, spec=spec)
            with zipfile.ZipFile(root / 'browser.zip', 'w') as archive:
                assert len(packager.add_chromium(archive, browser)) == 3
            spec['sha256'] = '0' * 64
            retry = root / 'corrupt-stage'
            retry.mkdir()
            try:
                packager.provision_chromium(retry, package, spec=spec)
            except ValueError:
                assert not list((retry / 'chromium').iterdir())
            else:
                raise AssertionError('Corrupted browser accepted')
        else:
            try:
                packager.provision_chromium(stage, package, spec=spec)
            except ValueError:
                assert not list((stage / 'chromium').iterdir())
            else:
                raise AssertionError('Invalid browser path accepted')
print('Automatic browser provisioning, hash and Windows-path checks passed.')

with tempfile.TemporaryDirectory(prefix='waypoint-vcrt-bundle-fixture-', dir=target) as temp:
    root = Path(temp)
    files = {}
    for name in packager.RUNTIME_CATALOG['msvc']['files']:
        content = b'fictional runtime ' + name.encode()
        (root / name).write_bytes(content)
        files[name] = hashlib.sha256(content).hexdigest()
    catalog = dict(packager.RUNTIME_CATALOG)
    catalog['msvc'] = dict(catalog['msvc'], files=files)
    with patch.object(packager, 'RUNTIME_CATALOG', catalog):
        with zipfile.ZipFile(root / 'vcrt.zip', 'w') as archive:
            inventory = packager.add_vcrt(archive, root, with_office=True, with_ollama=True)
            assert len(inventory) == 36
        with zipfile.ZipFile(root / 'vcrt.zip') as archive:
            for name, digest in inventory.items():
                assert hashlib.sha256(archive.read('app/' + name)).hexdigest() == digest
            assert 'app/licenses/MSVC_RUNTIME_LICENSE.docx' in archive.namelist()
        (root / 'msvcp140.dll').write_bytes(b'damaged')
        try:
            with zipfile.ZipFile(root / 'damaged.zip', 'w') as archive:
                packager.add_vcrt(archive, root, with_office=True, with_ollama=True)
        except ValueError:
            pass
        else:
            raise AssertionError('Damaged Microsoft runtime accepted')
print('App-local Microsoft runtime and original license checks passed.')

# Exercise the pinned-runtime bundler using tiny fictional archives.
with tempfile.TemporaryDirectory(prefix='waypoint-ollama-fixture-', dir=target) as temp:
    root = Path(temp)
    license_file = root / 'LICENSE'
    license_file.write_bytes(b'fictional license')
    base = root / 'base.zip'
    rocm = root / 'rocm.zip'
    def make_archive(path, entries):
        with zipfile.ZipFile(path, 'w') as source:
            for name, data in entries:
                source.writestr(name, data)
    def archive_spec(path):
        data = path.read_bytes()
        return (path.name, 'unused', len(data), hashlib.sha256(data).hexdigest(), 1024 * 1024)
    make_archive(base, [('ollama.exe', b'executable'), ('lib/ollama/LLAMA_CPP_LICENSE', b'license'), ('lib/ollama/vulkan/ggml-vulkan.dll', b'vulkan')])
    make_archive(rocm, [('lib/ollama/rocm_v7_1/ggml-hip.dll', b'rocm')])
    specs = (archive_spec(base), archive_spec(rocm))
    output = root / 'bundle.zip'
    with zipfile.ZipFile(output, 'w') as archive:
        hashes = packager.add_ollama_runtime(archive, root, specs=specs, license_path=license_file, license_hash=hashlib.sha256(b'fictional license').hexdigest())
    with zipfile.ZipFile(output) as archive:
        assert len(hashes) == 5
        for name, digest in hashes.items():
            assert hashlib.sha256(archive.read('app/' + name)).hexdigest() == digest
    with zipfile.ZipFile(root / 'collision.zip', 'w') as archive:
        archive.writestr('app/dependencies/ollama/OLLAMA.EXE', b'previously bundled')
        try:
            packager.add_ollama_runtime(archive, root, specs=specs, license_path=license_file, license_hash=hashlib.sha256(b'fictional license').hexdigest())
        except ValueError as error:
            assert 'overwrite' in str(error)
        else:
            raise AssertionError('A runtime overwrote an existing Windows-path alias')
    with zipfile.ZipFile(root / 'collision.zip') as archive:
        assert archive.namelist() == ['app/dependencies/ollama/OLLAMA.EXE']
        assert archive.read(archive.namelist()[0]) == b'previously bundled'
    make_archive(rocm, [('lib/ollama/rocm_v7_1/ggml-hip.dll', b'changed')])
    try:
        with zipfile.ZipFile(root / 'bad-hash.zip', 'w') as archive:
            packager.add_ollama_runtime(archive, root, specs=specs, license_path=license_file, license_hash=hashlib.sha256(b'fictional license').hexdigest())
    except ValueError as error:
        assert 'pinned' in str(error)
    else:
        raise AssertionError('A changed runtime archive was accepted')
    make_archive(rocm, [('../escape.dll', b'bad')])
    try:
        with zipfile.ZipFile(root / 'bad-path.zip', 'w') as archive:
            packager.add_ollama_runtime(archive, root, specs=(archive_spec(base), archive_spec(rocm)), license_path=license_file, license_hash=hashlib.sha256(b'fictional license').hexdigest())
    except ValueError as error:
        assert 'path' in str(error)
    else:
        raise AssertionError('A traversal path was accepted')
    make_archive(rocm, [('OLLAMA.EXE', b'duplicate')])
    try:
        with zipfile.ZipFile(root / 'duplicate.zip', 'w') as archive:
            packager.add_ollama_runtime(archive, root, specs=(archive_spec(base), archive_spec(rocm)), license_path=license_file, license_hash=hashlib.sha256(b'fictional license').hexdigest())
    except ValueError as error:
        assert 'Duplicate' in str(error)
    else:
        raise AssertionError('A case-insensitive duplicate was accepted')
    make_archive(rocm, [('NUL.txt', b'device')])
    try:
        with zipfile.ZipFile(root / 'reserved.zip', 'w') as archive:
            packager.add_ollama_runtime(archive, root, specs=(archive_spec(base), archive_spec(rocm)), license_path=license_file, license_hash=hashlib.sha256(b'fictional license').hexdigest())
    except ValueError as error:
        assert 'path' in str(error)
    else:
        raise AssertionError('A Windows device path was accepted')
    try:
        with zipfile.ZipFile(root / 'bad-license.zip', 'w') as archive:
            packager.add_ollama_runtime(archive, root, specs=specs, license_path=license_file, license_hash='0'*64)
    except ValueError as error:
        assert 'license' in str(error)
    else:
        raise AssertionError('An unreviewed runtime license was accepted')
    make_archive(rocm, [('lib/ollama/rocm_v7_1/ggml-hip.dll', b'rocm')])
    previous = root / 'previous.zip'
    with zipfile.ZipFile(previous, 'w') as archive:
        inventory = packager.add_ollama_runtime(archive, root, specs=(archive_spec(base), archive_spec(rocm)))
        archive.writestr('app/package-manifest.json', __import__('json').dumps({'ollama_files': inventory}))
    previous_hash = hashlib.sha256(previous.read_bytes()).hexdigest()
    with zipfile.ZipFile(root / 'reused.zip', 'w') as archive:
        reused = packager.add_ollama_from_package(archive, previous, previous_hash)
    assert reused == inventory
    try:
        with zipfile.ZipFile(root / 'wrong-source.zip', 'w') as archive:
            packager.add_ollama_from_package(archive, previous, '0' * 64)
    except ValueError as error:
        assert 'SHA-256' in str(error)
    else:
        raise AssertionError('A previous package with the wrong hash was accepted')
    invalid_manifest = root / 'invalid-manifest.zip'
    with zipfile.ZipFile(invalid_manifest, 'w') as archive:
        archive.writestr('app/package-manifest.json', '[]')
    try:
        with zipfile.ZipFile(root / 'invalid-manifest-copy.zip', 'w') as archive:
            packager.add_ollama_from_package(archive, invalid_manifest, hashlib.sha256(invalid_manifest.read_bytes()).hexdigest())
    except ValueError as error:
        assert 'manifest is invalid' in str(error)
    else:
        raise AssertionError('An invalid prior manifest was accepted')
    changed_during_copy = root / 'changed-during-copy.zip'
    changed_during_copy.write_bytes(previous.read_bytes())
    class MutatingArchive:
        def __init__(self, wrapped): self.wrapped, self.changed = wrapped, False
        def namelist(self): return self.wrapped.namelist()
        def open(self, *args, **kwargs):
            inner = self.wrapped.open(*args, **kwargs)
            outer = self
            class FileContext:
                def __enter__(self): return inner.__enter__()
                def __exit__(self, *exc):
                    result = inner.__exit__(*exc)
                    if not outer.changed:
                        with changed_during_copy.open('ab') as source: source.write(b'changed')
                        outer.changed = True
                    return result
            return FileContext()
    try:
        with zipfile.ZipFile(root / 'changed-copy.zip', 'w') as archive:
            packager.add_ollama_from_package(MutatingArchive(archive), changed_during_copy, hashlib.sha256(changed_during_copy.read_bytes()).hexdigest())
    except ValueError as error:
        assert 'changed while' in str(error)
    else:
        raise AssertionError('A package changed during copying was accepted')
    unlisted = root / 'unlisted-previous.zip'
    with zipfile.ZipFile(previous) as source, zipfile.ZipFile(unlisted, 'w') as dest:
        for entry in source.infolist(): dest.writestr(entry.filename, source.read(entry))
        dest.writestr('app/dependencies/ollama/unlisted.dll', b'extra')
    try:
        with zipfile.ZipFile(root / 'unlisted-copy.zip', 'w') as archive:
            packager.add_ollama_from_package(archive, unlisted, hashlib.sha256(unlisted.read_bytes()).hexdigest())
    except ValueError as error:
        assert 'do not match its inventory' in str(error)
    else:
        raise AssertionError('An unlisted previous runtime file was accepted')
    corrupted = root / 'corrupt-previous.zip'
    with zipfile.ZipFile(previous) as source, zipfile.ZipFile(corrupted, 'w') as dest:
        for entry in source.infolist():
            value = source.read(entry)
            if entry.filename == 'app/dependencies/ollama/ollama.exe': value = b'altered'
            dest.writestr(entry.filename, value)
    try:
        with zipfile.ZipFile(root / 'corrupt-copy.zip', 'w') as archive:
            packager.add_ollama_from_package(archive, corrupted, hashlib.sha256(corrupted.read_bytes()).hexdigest())
    except ValueError as error:
        assert 'does not match its inventory' in str(error)
    else:
        raise AssertionError('A damaged previous runtime was accepted')
print('Fictional Ollama bundle integrity checks passed.')
