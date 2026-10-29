"""Exercise optional converter bundling with fictional, small local files."""
import hashlib
import importlib.util
import io
import os
import sys
from pathlib import Path
import tempfile
import zipfile

script = Path(__file__).with_name('package-windows.py')
spec = importlib.util.spec_from_file_location('waypoint_packager', script)
packager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packager)

target = script.resolve().parents[3] / 'target'
target.mkdir(exist_ok=True)
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
    assert packager.select_libreoffice(False, None, env) == program_files / 'LibreOffice'
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
