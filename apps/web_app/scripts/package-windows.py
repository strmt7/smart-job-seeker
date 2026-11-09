"""Create a new local Windows prototype bundle and collect third-party license files."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
import urllib.request
import zipfile

REPO = Path(__file__).resolve().parents[3]
TARGET = 'x86_64-pc-windows-msvc'
PYTHON_URL = 'https://api.nuget.org/v3-flatcontainer/python/3.12.10/python.3.12.10.nupkg'
PYTHON_SHA256 = '0eb85c2dfccccf1b17352de4c397f69194035b7d37149eacc16f1147d93de3b8'
PDF_WHEELS = (
    ('pypdf-6.10.0-py3-none-any.whl', 'https://files.pythonhosted.org/packages/55/f2/7ebe366f633f30a6ad105f650f44f24f98cb1335c4157d21ae47138b3482/pypdf-6.10.0-py3-none-any.whl', '90005e959e1596c6e6c84c8b0ad383285b3e17011751cedd17f2ce8fcdfc86de'),
    ('pypdfium2-5.14.0-py3-none-win_amd64.whl', 'https://files.pythonhosted.org/packages/00/dc/ca7874924c9cfd701ad53f89529968523790e70473e0b71e834668316148/pypdfium2-5.14.0-py3-none-win_amd64.whl', '149fd5c6397b8df8bf7911a93506eff0be874f877afe7ac936cf5d37d21a6a06'),
    ('pillow-12.3.0-cp312-cp312-win_amd64.whl', 'https://files.pythonhosted.org/packages/45/89/da2f7971a317f83d807fdd4065c0af40208e59e692cc43d315a71a0e96d1/pillow-12.3.0-cp312-cp312-win_amd64.whl', 'a2b55dd6b2a4c4b7d87ffa56bdb33fdc5fdb9a462173861a7bc097f17d91cb09'),
)
# TheDocumentFoundation's published checksum and the winget x64 installer
# manifest for 26.8.2.2 agree on this exact 26.8.2 MSI.
OFFICE_MSI_URL = 'https://download.documentfoundation.org/libreoffice/stable/26.8.2/win/x86_64/LibreOffice_26.8.2_Win_x86-64.msi'
OFFICE_MSI_SHA256 = 'ff4b911403b67ee9e6249f820f9782c44bb32c94fdaa45b0da53a8e419e5307a'
OFFICE_MSI_MAX_BYTES = 512 * 1024 * 1024
OLLAMA_ARCHIVES = (
    ('ollama-windows-amd64.zip', 'https://github.com/ollama/ollama/releases/download/v0.40.13/ollama-windows-amd64.zip', 1601969847, '7afa38624fa62ee5d8293422d0ab084c1bb4603134c7f4dafce11a033b57cc19', 4 * 1024**3),
    ('ollama-windows-amd64-rocm.zip', 'https://github.com/ollama/ollama/releases/download/v0.40.13/ollama-windows-amd64-rocm.zip', 257076385, '2ab082d5414d51ef4cd675543c976770743c90e3cad0d9deb540160ae3b25573', 2 * 1024**3),
)
OLLAMA_LICENSE_SHA256 = '5934ed2ce0d15154bcdb9c85203210abac0da4314af34081e36df4599f90b226'

def discover_libreoffice(environment):
    """Find a local candidate; bundling still validates and inventories it."""
    candidates = []
    configured = environment.get('WAYPOINT_SOFFICE')
    if configured:
        executable = Path(configured)
        if executable.name.lower() == 'soffice.com' and executable.parent.name.lower() == 'program':
            candidates.append(executable.parent.parent)
    for entry in environment.get('PATH', '').split(os.pathsep):
        if entry:
            directory = Path(entry)
            if directory.name.lower() == 'program':
                candidates.append(directory.parent)
    for key in ('PROGRAMFILES', 'PROGRAMW6432', 'PROGRAMFILES(X86)'):
        if environment.get(key):
            candidates.append(Path(environment[key]) / 'LibreOffice')
    if environment.get('LOCALAPPDATA'):
        candidates.append(Path(environment['LOCALAPPDATA']) / 'Programs' / 'LibreOffice')
    for root in candidates:
        if ((root / 'program' / 'soffice.com').is_file()
            and any((root / name).is_file() for name in ('LICENSE', 'LICENSE.txt', 'LICENSE.html'))):
            return root
    return None

def select_libreoffice(no_libreoffice, configured_root, environment):
    """Select an existing converter; None also means provisioning is needed."""
    if no_libreoffice:
        return None
    root = Path(configured_root) if configured_root else discover_libreoffice(environment)
    return root

def copy_verified_office_msi(source, destination, expected_hash=OFFICE_MSI_SHA256):
    """Stream a bounded installer and refuse it before execution if it differs."""
    digest = hashlib.sha256()
    size = 0
    with destination.open('xb') as out:
        while chunk := source.read(1024 * 1024):
            size += len(chunk)
            if size > OFFICE_MSI_MAX_BYTES:
                raise ValueError('LibreOffice installer exceeds the size limit')
            digest.update(chunk)
            out.write(chunk)
    if not size or digest.hexdigest() != expected_hash:
        raise ValueError('LibreOffice installer does not match the pinned SHA-256')

def provision_libreoffice(stage, local_msi=None):
    """Extract a verified official MSI to a private build directory, not the host."""
    if sys.platform != 'win32':
        raise ValueError('Automatic LibreOffice provisioning requires Windows')
    if shutil.disk_usage(stage).free < 4 * 1024 * 1024 * 1024:
        raise ValueError('At least 4 GiB of free disk space is required for converter provisioning')
    msi = stage / 'waypoint-libreoffice.msi'
    try:
        source = open(local_msi, 'rb') if local_msi else urllib.request.urlopen(OFFICE_MSI_URL, timeout=60)
        with source:
            copy_verified_office_msi(source, msi)
    except BaseException:
        # The installer has not started yet, so only our partial download can
        # exist here. Avoid retaining hundreds of MiB after a failed transfer.
        msi.unlink(missing_ok=True)
        raise
    office = stage / 'office'
    office.mkdir()
    try:
        result = subprocess.run(
            [str(Path(os.environ['SystemRoot']) / 'System32' / 'msiexec.exe'), '/a', str(msi), '/qn', '/norestart', f'TARGETDIR={office}'],
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            creationflags=subprocess.CREATE_NO_WINDOW, timeout=300, check=False,
        )
    except subprocess.TimeoutExpired as error:
        # The Windows Installer service may still be active. Do not remove the
        # stage after a timeout; the caller reports its location for inspection.
        raise ValueError('LibreOffice extraction timed out; Windows Installer may still be active') from error
    if result.returncode != 0:
        raise ValueError(f'LibreOffice extraction failed with exit code {result.returncode}')
    if not (office / 'program' / 'soffice.com').is_file() or not (office / 'LICENSE.html').is_file():
        raise ValueError('Extracted LibreOffice distribution is incomplete')
    # Administrative extraction also produces a transformed MSI. It is not
    # needed by the converter, and this is a file in our private stage only.
    (office / msi.name).unlink(missing_ok=True)
    return office

def check_windows_bundle_path(relative, seen, component):
    """Reject paths that cannot be reliably extracted onto Windows."""
    parts = relative.split('/')
    key = relative.casefold()
    if (any(part in ('', '.', '..') or part.endswith(('.', ' ')) for part in parts)
        or any(re.fullmatch(r'(?i)(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?', part) for part in parts)
        or re.search(r'[\\:*?"<>|\x00-\x1f]', relative)
        or key in seen):
        raise ValueError(f'Unsupported {component} file path: {relative}')
    seen.add(key)

def add_libreoffice(archive, root):
    """Bundle an existing complete distribution without modifying the host."""
    root = Path(root).resolve(strict=True)
    if not root.is_dir() or not (root / 'program' / 'soffice.com').is_file():
        raise ValueError('LibreOffice directory must contain program/soffice.com')
    if not any((root / name).is_file() for name in ('LICENSE', 'LICENSE.txt', 'LICENSE.html')):
        raise ValueError('LibreOffice distribution must include its license')
    files, seen = [], set()
    total = 0
    for path in root.rglob('*'):
        if path.is_symlink() or getattr(path, 'is_junction', lambda: False)():
            raise ValueError('LibreOffice distribution contains a link or junction')
        if not path.is_file():
            continue
        relative = path.relative_to(root).as_posix()
        check_windows_bundle_path(relative, seen, 'LibreOffice')
        total += path.stat().st_size
        files.append((path, relative))
        if len(files) > 50000 or total > 2 * 1024 * 1024 * 1024:
            raise ValueError('LibreOffice distribution exceeds package limits')
    hashes = {}
    for path, relative in sorted(files):
        destination = 'app/dependencies/libreoffice/' + relative
        sha = hashlib.sha256()
        with path.open('rb') as src, archive.open(destination, 'w') as dest:
            while chunk := src.read(1024 * 1024):
                sha.update(chunk)
                dest.write(chunk)
        hashes['dependencies/libreoffice/' + relative] = sha.hexdigest()
    return hashes


def add_chromium(archive, root):
    """Bundle a supplied browser and its notices without changing the host."""
    root = Path(root).resolve(strict=True)
    if not root.is_dir() or not (root / 'chrome.exe').is_file():
        raise ValueError('Chromium directory must contain chrome.exe')
    if not (root / 'LICENSES.chromium.html').is_file():
        raise ValueError('Chromium directory must contain LICENSES.chromium.html')
    files, seen, total = [], set(), 0
    for path in root.rglob('*'):
        if path.is_symlink() or getattr(path, 'is_junction', lambda: False)():
            raise ValueError('Chromium distribution contains a link or junction')
        if not path.is_file():
            continue
        relative = path.relative_to(root).as_posix()
        check_windows_bundle_path(relative, seen, 'Chromium')
        total += path.stat().st_size
        files.append((path, relative))
        if len(files) > 50000 or total > 2 * 1024**3:
            raise ValueError('Chromium distribution exceeds package limits')
    hashes = {}
    for path, relative in sorted(files):
        name = 'dependencies/chromium/' + relative
        digest = hashlib.sha256()
        with path.open('rb') as src, archive.open('app/' + name, 'w') as dest:
            while chunk := src.read(1024 * 1024):
                digest.update(chunk)
                dest.write(chunk)
        hashes[name] = digest.hexdigest()
    return hashes

def add_pdf_wheels(archive, wheel_dir=None):
    for name, url, expected_hash in PDF_WHEELS:
        with tempfile.TemporaryFile() as temporary:
            source = open(Path(wheel_dir) / name, 'rb') if wheel_dir else urllib.request.urlopen(url, timeout=60)
            sha = hashlib.sha256()
            size = 0
            with source:
                while chunk := source.read(1024 * 1024):
                    size += len(chunk)
                    if size > 50 * 1024 * 1024:
                        raise ValueError(f'{name} exceeds the size limit')
                    sha.update(chunk)
                    temporary.write(chunk)
            if sha.hexdigest() != expected_hash:
                raise ValueError(f'{name} does not match the pinned SHA-256')
            temporary.seek(0)
            with zipfile.ZipFile(temporary) as wheel:
                members = [entry for entry in wheel.infolist() if not entry.is_dir()]
                if sum(entry.file_size for entry in members) > 150 * 1024 * 1024:
                    raise ValueError(f'{name} contents exceed the size limit')
                for entry in members:
                    relative = entry.filename
                    if '\\' in relative or any(part in ('', '.', '..') for part in relative.split('/')) or '.data/' in relative:
                        raise ValueError(f'Unexpected wheel path in {name}')
                    dest_name = 'app/dependencies/python/Lib/site-packages/' + relative
                    if dest_name in archive.NameToInfo:
                        raise ValueError(f'Duplicate bundled Python file: {relative}')
                    with wheel.open(entry) as src, archive.open(dest_name, 'w') as dest:
                        shutil.copyfileobj(src, dest, length=1024 * 1024)

def add_python_runtime(archive, package_path=None):
    # Stream a pinned, official x64 distribution into the package; do not
    # extract it into or modify the developer's Python installation.
    with tempfile.TemporaryFile() as temporary:
        if package_path:
            source = open(package_path, 'rb')
        else:
            source = urllib.request.urlopen(PYTHON_URL, timeout=60)
        sha = hashlib.sha256()
        size = 0
        with source:
            while chunk := source.read(1024 * 1024):
                size += len(chunk)
                if size > 50 * 1024 * 1024:
                    raise ValueError('Python runtime package exceeds the size limit')
                sha.update(chunk)
                temporary.write(chunk)
        if sha.hexdigest() != PYTHON_SHA256:
            raise ValueError('Python runtime package does not match the pinned SHA-256')
        temporary.seek(0)
        with zipfile.ZipFile(temporary) as source_zip:
            members = [entry for entry in source_zip.infolist() if entry.filename.startswith('tools/') and not entry.is_dir()]
            if sum(entry.file_size for entry in members) > 150 * 1024 * 1024:
                raise ValueError('Python runtime contents exceed the size limit')
            names = {entry.filename for entry in members}
            if not {'tools/python.exe', 'tools/LICENSE.txt', 'tools/Lib/ensurepip/__init__.py'}.issubset(names):
                raise ValueError('Python runtime package is incomplete')
            for entry in members:
                relative = entry.filename.removeprefix('tools/')
                if '\\' in relative or any(part in ('', '.', '..') for part in relative.split('/')):
                    raise ValueError('Unexpected Python runtime package path')
                with source_zip.open(entry) as src, archive.open('app/dependencies/python/' + relative, 'w') as dest:
                    shutil.copyfileobj(src, dest, length=1024 * 1024)

def add_ollama_runtime(archive, local_archives=None, *, specs=OLLAMA_ARCHIVES, license_path=None, license_hash=OLLAMA_LICENSE_SHA256):
    """Stream pinned official archives into an app-local runtime inventory."""
    license_path = Path(license_path) if license_path else REPO / 'docs/OLLAMA_LICENSE.txt'
    license_bytes = license_path.read_bytes().replace(b'\r\n', b'\n')
    if hashlib.sha256(license_bytes).hexdigest() != license_hash:
        raise ValueError('Ollama license does not match the reviewed version')
    hashes = {}
    seen = set()
    for name, url, expected_size, expected_hash, max_unpacked in specs:
        with tempfile.TemporaryFile(dir=REPO / 'target') as temporary:
            source = open(Path(local_archives) / name, 'rb') if local_archives else urllib.request.urlopen(url, timeout=60)
            sha = hashlib.sha256()
            size = 0
            with source:
                while chunk := source.read(1024 * 1024):
                    size += len(chunk)
                    if size > expected_size:
                        raise ValueError(f'{name} exceeds its pinned size')
                    sha.update(chunk)
                    temporary.write(chunk)
            if size != expected_size or sha.hexdigest() != expected_hash:
                raise ValueError(f'{name} does not match its pinned size and SHA-256')
            temporary.seek(0)
            with zipfile.ZipFile(temporary) as source_zip:
                entries = source_zip.infolist()
                if len(entries) > 50000 or sum(entry.file_size for entry in entries) > max_unpacked:
                    raise ValueError(f'{name} contents exceed runtime limits')
                for entry in entries:
                    if entry.is_dir():
                        continue
                    relative = entry.filename
                    parts = relative.split('/')
                    if (not relative or '\\' in relative or any(part in ('', '.', '..') or part.endswith(('.', ' ')) for part in parts)
                        or any(re.fullmatch(r'(?i)(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?', part) for part in parts)
                        or re.search(r'[\x00-\x1f:*?"<>|]', relative)
                        or entry.flag_bits & 1 or ((entry.external_attr >> 16) & 0o170000) == 0o120000):
                        raise ValueError(f'Unexpected Ollama archive path in {name}')
                    key = relative.casefold()
                    if key in seen or key == 'license':
                        raise ValueError(f'Duplicate Ollama runtime path in {name}')
                    seen.add(key)
                    destination = 'app/dependencies/ollama/' + relative
                    digest = hashlib.sha256()
                    with source_zip.open(entry) as src, archive.open(destination, 'w', force_zip64=True) as dest:
                        while chunk := src.read(1024 * 1024):
                            digest.update(chunk)
                            dest.write(chunk)
                    hashes['dependencies/ollama/' + relative] = digest.hexdigest()
    required = {'ollama.exe', 'lib/ollama/llama_cpp_license', 'lib/ollama/vulkan/ggml-vulkan.dll', 'lib/ollama/rocm_v7_1/ggml-hip.dll'}
    if not required.issubset(seen):
        raise ValueError('Ollama runtime archives are incomplete')
    archive.writestr('app/dependencies/ollama/LICENSE', license_bytes)
    hashes['dependencies/ollama/LICENSE'] = hashlib.sha256(license_bytes).hexdigest()
    return hashes


def add_ollama_from_package(archive, package_path, expected_hash):
    """Reuse a caller-pinned package while rechecking every runtime file."""
    if not re.fullmatch(r'[a-fA-F0-9]{64}', expected_hash or ''):
        raise ValueError('A SHA-256 is required to reuse an Ollama package')
    package_path = Path(package_path)
    with package_path.open('rb') as source:
        digest = hashlib.sha256()
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
        if digest.hexdigest() != expected_hash.lower():
            raise ValueError('The previous package does not match the specified SHA-256')
        source.seek(0)
        with zipfile.ZipFile(source) as previous:
            try:
                manifest_info = previous.getinfo('app/package-manifest.json')
            except KeyError as error:
                raise ValueError('The previous package manifest is missing') from error
            if manifest_info.file_size > 8 * 1024 * 1024:
                raise ValueError('The previous package manifest exceeds the size limit')
            manifest = json.loads(previous.read(manifest_info))
            if not isinstance(manifest, dict):
                raise ValueError('The previous package manifest is invalid')
            files = manifest.get('ollama_files')
            if not isinstance(files, dict) or not 1 <= len(files) <= 50000:
                raise ValueError('The previous package lacks a bounded Ollama inventory')
            seen = set()
            for name, value in files.items():
                if not isinstance(name, str) or not name.startswith('dependencies/ollama/') or not isinstance(value, str) or not re.fullmatch(r'[a-fA-F0-9]{64}', value):
                    raise ValueError('The previous Ollama inventory is invalid')
                relative = name.removeprefix('dependencies/ollama/')
                parts = relative.split('/')
                if (not relative or '\\' in relative or any(part in ('', '.', '..') or part.endswith(('.', ' ')) for part in parts)
                    or any(re.fullmatch(r'(?i)(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?', part) for part in parts)
                    or re.search(r'[\x00-\x1f:*?"<>|]', relative) or relative.casefold() in seen):
                    raise ValueError('The previous Ollama inventory contains an invalid path')
                seen.add(relative.casefold())
            required = {'ollama.exe', 'license', 'lib/ollama/llama_cpp_license', 'lib/ollama/vulkan/ggml-vulkan.dll', 'lib/ollama/rocm_v7_1/ggml-hip.dll'}
            if not required.issubset(seen):
                raise ValueError('The previous Ollama inventory is incomplete')
            if files.get('dependencies/ollama/LICENSE', '').lower() != OLLAMA_LICENSE_SHA256:
                raise ValueError('The previous package has an unexpected Ollama license')
            prefix = 'app/dependencies/ollama/'
            entries = [item for item in previous.infolist() if item.filename.startswith(prefix) and not item.is_dir()]
            names = [item.filename.removeprefix('app/') for item in entries]
            if len(entries) != len(files) or len(set(name.casefold() for name in names)) != len(entries) or set(names) != set(files):
                raise ValueError('The previous Ollama files do not match its inventory')
            if sum(item.file_size for item in entries) > 6 * 1024**3:
                raise ValueError('The previous Ollama runtime exceeds the size limit')
            for item, name in zip(entries, names):
                if item.flag_bits & 1 or ((item.external_attr >> 16) & 0o170000) == 0o120000:
                    raise ValueError('The previous Ollama runtime contains an unsupported entry')
                sha = hashlib.sha256()
                with previous.open(item) as src, archive.open('app/' + name, 'w', force_zip64=True) as dest:
                    while chunk := src.read(1024 * 1024):
                        sha.update(chunk)
                        dest.write(chunk)
                if sha.hexdigest() != files[name].lower():
                    raise ValueError('A previous Ollama runtime file does not match its inventory')
        source.seek(0)
        final_digest = hashlib.sha256()
        while chunk := source.read(1024 * 1024):
            final_digest.update(chunk)
        if final_digest.hexdigest() != expected_hash.lower():
            raise ValueError('The previous package changed while it was being copied')
        return files


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', required=True, help='New ZIP path inside this repository')
    parser.add_argument('--release', action='store_true')
    parser.add_argument('--build-dir', help='Directory containing the executable and its build receipt (defaults to target/<profile>)')
    parser.add_argument('--python-package', help='Optional local copy of the pinned Python NuGet package for offline packaging')
    parser.add_argument('--wheel-dir', help='Optional directory of pinned PDF wheels for offline packaging')
    parser.add_argument('--chromium-dir', help='Optional complete, redistributable Chromium directory with license notices')
    ollama = parser.add_mutually_exclusive_group()
    ollama.add_argument('--ollama-archive-dir', help='Optional directory of pinned Ollama archives for offline packaging')
    ollama.add_argument('--no-ollama', action='store_true', help='Do not bundle the local model runtime')
    ollama.add_argument('--ollama-from-package', help='Reuse the local runtime from a previously verified package')
    parser.add_argument('--ollama-package-sha256', help='Required SHA-256 of the previous package')
    office = parser.add_mutually_exclusive_group()
    office.add_argument('--libreoffice-dir', help='Bundle this complete LibreOffice distribution for PDF conversion')
    office.add_argument('--libreoffice-msi', help='Use a local copy of the pinned LibreOffice MSI for offline provisioning')
    office.add_argument('--no-libreoffice', action='store_true', help='Do not automatically bundle an installed LibreOffice distribution')
    parser.add_argument('--openssl-root', default=os.environ.get('OPENSSL_DIR'), help='OpenSSL installation used for the build')
    args = parser.parse_args()
    if bool(args.ollama_from_package) != bool(args.ollama_package_sha256):
        raise ValueError('--ollama-from-package and --ollama-package-sha256 must be used together')
    if not args.no_ollama:
        target_dir = REPO / 'target'
        target_dir.mkdir(exist_ok=True)
        if shutil.disk_usage(target_dir).free < 8 * 1024**3:
            raise ValueError('At least 8 GiB of free disk space is required for runtime bundling')
    office_root = select_libreoffice(args.no_libreoffice, args.libreoffice_dir, os.environ)
    output = Path(args.output).resolve()
    if not output.is_relative_to(REPO) or output.suffix.lower() != '.zip' or output.exists():
        raise ValueError('Output must be a new .zip path inside the repository')
    profile = 'release' if args.release else 'debug'
    build = Path(args.build_dir).resolve() if args.build_dir else REPO / 'target' / profile
    exe = build / 'waypoint_web.exe'
    dlls = list(build.glob('libcrypto*.dll'))
    if not exe.is_file() or len(dlls) != 1:
        raise ValueError('Build Smart Jobseeker first and ensure exactly one libcrypto DLL is next to the executable')
    receipt_path = build / 'waypoint-build.json'
    if not receipt_path.is_file(): raise ValueError('Build receipt is missing; use build-windows.ps1 first')
    receipt = json.loads(receipt_path.read_text(encoding='utf-8-sig'))
    digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    if (receipt.get('profile') != profile or receipt.get('sqlcipher') is not True
        or receipt.get('executable_sha256') != digest(exe)
        or receipt.get('openssl_dll') != dlls[0].name
        or receipt.get('openssl_sha256') != digest(dlls[0])
        or receipt.get('cargo_lock_sha256') != digest(REPO / 'Cargo.lock')):
        raise ValueError('Build receipt does not match the executable, runtime or Cargo.lock; rebuild first')
    program_files = Path(os.environ.get('ProgramFiles', 'C:/Program Files'))
    # The build may have run in another process with a custom OpenSSL root.
    # Explicit overrides take precedence over the path recorded in its receipt.
    roots = ([Path(args.openssl_root)] if args.openssl_root else
             [Path(receipt['openssl_root'])] if isinstance(receipt.get('openssl_root'), str) and receipt['openssl_root'] else
             [program_files / 'OpenSSL-Win64', program_files / 'OpenSSL'])
    matching = [root for root in roots if (root / 'bin' / dlls[0].name).is_file()
                and digest(root / 'bin' / dlls[0].name) == digest(dlls[0])
                and (root / 'license.txt').is_file()]
    if not matching:
        raise ValueError('OpenSSL runtime does not match the specified license source')
    openssl_license = matching[0] / 'license.txt'
    tree = subprocess.check_output(['cargo','tree','-p','waypoint_web','-e','normal','--target',TARGET,'--offline','--prefix','none','--format','{p}'],cwd=REPO,text=True,encoding='utf-8')
    registry = Path(os.environ.get('CARGO_HOME') or (Path(os.environ['USERPROFILE']) / '.cargo')) / 'registry' / 'src'
    packages = {}
    for line in tree.splitlines():
        match = re.match(r'^([^ ]+) v([^ ]+)',line)
        if not match:
            raise ValueError('Unexpected Cargo dependency output')
        name, version = match.groups()
        if (name, version) in packages: continue
        candidates = list(registry.glob(f'*/{name}-{version}/Cargo.toml'))
        if not candidates:
            if '(' in line and str(REPO).casefold() in line.casefold(): continue
            raise ValueError(f'No local manifest for dependency {name} {version}')
        if len(candidates) != 1: raise ValueError(f'Ambiguous local registry source for {name} {version}')
        root = candidates[0].parent
        package = tomllib.loads(candidates[0].read_text(encoding='utf-8')).get('package',{})
        files = {f.resolve() for f in root.iterdir() if f.is_file() and f.name.lower().startswith(('license','licence','copying','notice'))}
        # The published crate omits its license file. This is the upstream
        # LICENSE blob ca4e06058707e187b049ee4da06ed553b1854243 at the crate's
        # .cargo_vcs_info.json revision 14011d08c42a7cd368698fe28f77eb4cd0b65bf0.
        if (name, version) == ('mutants', '0.0.4') and not files:
            files.add((REPO / 'docs/MUTANTS_LICENSE.txt').resolve())
        # The 0.1.1 crate omits license files. This MIT license is from the
        # repository commit recorded in its .cargo_vcs_info.json.
        if (name, version) == ('gpuviewer-core', '0.1.1') and not files:
            fallback = (REPO / 'docs/GPUVIEWER_CORE_LICENSE.txt').resolve()
            if hashlib.sha256(fallback.read_bytes().replace(b'\r\n', b'\n')).hexdigest() != 'd82bb4454c3439167bdb64d9dd598d391978133cac416d2f417e6ef9450ab398':
                raise ValueError('The gpuviewer-core license does not match its upstream revision')
            files.add(fallback)
        if package.get('license-file'):
            license_file = (root / package['license-file']).resolve()
            if not license_file.is_relative_to(root.resolve()) or not license_file.is_file(): raise ValueError(f'Invalid license file for {name} {version}')
            files.add(license_file)
        if not files: raise ValueError(f'No local license text for {name} {version}')
        packages[(name,version)] = (package.get('license'), sorted(files))
    output.parent.mkdir(parents=True,exist_ok=True)
    office_stage = None
    if not args.no_libreoffice and (args.libreoffice_msi or office_root is None):
        target_dir = (REPO / 'target').resolve()
        office_stage = Path(tempfile.mkdtemp(prefix='waypoint-office-', dir=target_dir)).resolve()
        if office_stage.parent != target_dir:
            raise ValueError('Invalid converter staging directory')
        print('Provisioning the pinned LibreOffice converter into an isolated build directory.')
        try:
            office_root = provision_libreoffice(office_stage, args.libreoffice_msi)
        except (OSError, ValueError) as error:
            # Preserve the directory on failure, especially if the installer
            # service may still be running after a timeout.
            raise ValueError(f'{error}; converter staging retained at {office_stage}') from error
    if office_root is not None:
        print(f'Bundling LibreOffice from {office_root}; this can substantially increase package size.')
    else:
        print('LibreOffice bundling was explicitly disabled; document conversion will require separate setup.')
    # Exclusive creation avoids replacing an existing artifact; remove a partial ZIP on failure.
    created = False
    try:
        raw = output.open('x+b')
        created = True
        with raw, zipfile.ZipFile(raw,'w',zipfile.ZIP_DEFLATED,strict_timestamps=False) as archive:
            def add(path, dest): archive.write(path, dest)
            add(exe,'app/SmartJobseeker.exe');add(dlls[0],f'app/{dlls[0].name}')
            add_python_runtime(archive, args.python_package)
            add_pdf_wheels(archive, args.wheel_dir)
            office_hashes = add_libreoffice(archive, office_root) if office_root else {}
            chromium_hashes = add_chromium(archive, args.chromium_dir) if args.chromium_dir else {}
            if args.no_ollama:
                ollama_hashes = {}
            elif args.ollama_from_package:
                ollama_hashes = add_ollama_from_package(archive, args.ollama_from_package, args.ollama_package_sha256)
            else:
                ollama_hashes = add_ollama_runtime(archive, args.ollama_archive_dir)
            runtime_hashes = {}
            for info in archive.infolist():
                if info.filename.startswith('app/dependencies/python/'):
                    digest_runtime = hashlib.sha256()
                    with archive.open(info) as runtime_file:
                        while chunk := runtime_file.read(1024 * 1024):
                            digest_runtime.update(chunk)
                    runtime_hashes[info.filename.removeprefix('app/')] = digest_runtime.hexdigest()
            add(REPO/'apps/web_app/scripts/install-package.ps1','install.ps1')
            add(REPO/'apps/web_app/README.md','app/README.md')
            add(REPO/'docs/WEB_PROTOTYPE_CHECKPOINT.md','app/docs/WEB_PROTOTYPE_CHECKPOINT.md')
            add(receipt_path,'app/build-receipt.json')
            add(REPO/'LICENSE','app/licenses/waypoint-LICENSE')
            add(REPO/'docs/SQLCIPHER_LICENSE.txt','app/licenses/sqlcipher-LICENSE.txt')
            add(openssl_license,'app/licenses/openssl-LICENSE.txt')
            inventory=[]
            for (name,version),(license_expr,files) in sorted(packages.items()):
                for index,path in enumerate(files): add(path,f'app/licenses/crates/{name}-{version}/{index}-{path.name}')
                inventory.append({'name':name,'version':version,'license':license_expr,'files':len(files)})
            archive.writestr('app/licenses/dependencies.json',json.dumps(inventory,indent=2))
            archive.writestr('app/package-manifest.json',json.dumps({'profile':profile,'executable_sha256':hashlib.sha256(exe.read_bytes()).hexdigest(),'openssl_dll':dlls[0].name,'openssl_sha256':hashlib.sha256(dlls[0].read_bytes()).hexdigest(),'static_crt_requested':receipt.get('static_crt_requested') is True,'python_files':runtime_hashes,'libreoffice_files':office_hashes,'ollama_files':ollama_hashes,'chromium_files':chromium_hashes,'note':'Prototype bundle. Website interactions can transmit or submit data; do not interact with real forms when an application must not be sent.'},indent=2))
            office_note = 'LibreOffice is bundled.' if office_hashes else 'LibreOffice is not bundled and needs separate setup for document conversion.'
            ollama_note = 'Ollama is bundled; start it from Settings.' if ollama_hashes else 'Ollama is not bundled and needs separate setup.'
            browser_note = 'A supplied Chromium browser is bundled.' if chromium_hashes else 'A Chromium browser needs separate installation.'
            archive.writestr('INSTALL.txt',f'Extract the entire archive. On Windows, run install.ps1 in PowerShell for a per-user copy, or launch app/SmartJobseeker.exe directly. Python and PDF libraries are included. {office_note} {ollama_note} {browser_note} Microsoft Visual C++ runtime may need separate installation. See app/README.md.\n')
    except BaseException:
        if created and output.is_file(): output.unlink()
        raise
    finally:
        if office_stage is not None:
            target_dir = (REPO / 'target').resolve()
            resolved_stage = office_stage.resolve(strict=True)
            if (resolved_stage != office_stage or resolved_stage.parent != target_dir
                or not resolved_stage.name.startswith('waypoint-office-')):
                raise ValueError('Refusing to remove an unexpected converter staging directory')
            shutil.rmtree(resolved_stage)
    print(f'Created {output} with {len(packages)} dependency license inventories.')

if __name__ == '__main__':
    try: main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f'Packaging failed: {error}',file=sys.stderr);sys.exit(1)
