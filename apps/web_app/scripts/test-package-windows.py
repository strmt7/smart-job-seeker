"""Exercise optional converter bundling with fictional, small local files."""
import hashlib
import importlib.util
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
    output = Path(temp) / 'bundle.zip'
    with zipfile.ZipFile(output, 'w') as archive:
        inventory = packager.add_libreoffice(archive, root)
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
