"""Private runtime repair using the same pinned distributions as packaging.

Executed by the native app with its own Python in a bounded hidden Windows job.
Only the native caller creates this fresh, private source/staging tree.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys


class DirectoryArchive:
    """Stream the packager's validated archive members directly into a fresh tree."""
    def __init__(self, root, check_path):
        self.root = root
        self.check_path = check_path
        self.NameToInfo = {}
        self.seen = set()

    def namelist(self):
        return list(self.NameToInfo)

    def destination(self, name):
        if not name.startswith('app/'):
            raise ValueError('Runtime output must stay inside its app directory')
        self.check_path(name, self.seen, 'runtime')
        if len(self.seen) > 50000:
            raise ValueError('Runtime inventory exceeds its entry limit')
        destination = self.root / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        self.NameToInfo[name] = True
        return destination

    def open(self, name, mode='w', **_options):
        if mode != 'w':
            raise ValueError('Runtime sink only supports new writes')
        return self.destination(name).open('xb')

    def write(self, source, name):
        with Path(source).open('rb') as src, self.open(name) as dest:
            shutil.copyfileobj(src, dest, length=1024 * 1024)

    def writestr(self, name, data):
        with self.open(name) as dest:
            dest.write(data)


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def verify_inventory(output, hashes):
    if not hashes or len(hashes) > 50000:
        raise ValueError('Runtime inventory is empty or too large')
    actual = {p.relative_to(output / 'app').as_posix()
              for p in (output / 'app').rglob('*') if p.is_file()}
    if actual != set(hashes):
        raise ValueError('Runtime output differs from its complete inventory')
    for relative, expected in hashes.items():
        path = output / 'app' / relative
        if path.is_symlink() or not path.is_file() or digest(path) != expected:
            raise ValueError('Runtime readback differs from its verified inventory')


def probe(binary, tool, stage):
    environment = os.environ.copy()
    profile = stage / 'probe-profile'
    for name, relative in {'USERPROFILE':'user', 'APPDATA':'roaming', 'LOCALAPPDATA':'local',
                           'TEMP':'temp', 'TMP':'temp'}.items():
        directory = profile / relative
        directory.mkdir(parents=True, exist_ok=True)
        environment[name] = str(directory)
    if tool == 'ollama':
        # A version check must not contact the user's running Ollama service.
        environment['OLLAMA_HOST'] = '127.0.0.1:0'
    args = [str(binary), '--version']
    if tool == 'office':
        args = [str(binary), '-env:UserInstallation=' + (stage / 'office-profile').as_uri(),
                '--headless', '--version']
    completed = subprocess.run(args, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                               stderr=subprocess.STDOUT, timeout=30,
                               env=environment,
                               creationflags=subprocess.CREATE_NO_WINDOW, check=False)
    if completed.returncode or not completed.stdout or len(completed.stdout) > 16000:
        raise ValueError('The private runtime did not pass its version check')
    return completed.stdout.decode('utf-8', errors='replace').strip()[:1000]


def install(package, tool, stage):
    if tool not in {'browser', 'office', 'ollama'}:
        raise ValueError('Unknown runtime component')
    required = {'browser': 2, 'office': 4, 'ollama': 12}[tool] * 1024**3
    if shutil.disk_usage(stage).free < required:
        raise ValueError(f'At least {required // 1024**3} GiB free disk space is required')
    output = stage / 'output'
    output.mkdir()  # Never reuse or overwrite an existing installation.
    sink = DirectoryArchive(output, package.check_windows_bundle_path)
    work = stage / 'work'
    work.mkdir()
    if tool == 'browser':
        engine = package.provision_chromium(work)
        hashes = package.add_chromium(sink, engine)
        relative = 'dependencies/chromium/chrome-headless-shell.exe'
    else:
        if tool == 'office':
            office = package.provision_libreoffice(work)
            hashes = package.add_libreoffice(sink, office, replace_vcrt=True)
            relative = 'dependencies/libreoffice/program/soffice.com'
        else:
            hashes = package.add_ollama_runtime(sink)
            relative = 'dependencies/ollama/ollama.exe'
        runtime = package.provision_vcrt(work)
        hashes.update(package.add_vcrt(sink, runtime, with_office=tool == 'office',
                                      with_ollama=tool == 'ollama'))
        hashes['licenses/MSVC_RUNTIME_LICENSE.docx'] = package.RUNTIME_CATALOG['msvc']['license']['sha256']
    verify_inventory(output, hashes)
    version = probe(output / 'app' / relative, tool, stage)
    (output / 'inventory.json').write_text(json.dumps({'tool': tool, 'files': hashes,
                                                     'version': version}), encoding='utf-8')
    return {'tool': tool, 'relative': relative, 'version': version, 'files': len(hashes)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('tool', choices=['browser', 'office', 'ollama'])
    args = parser.parse_args()
    stage = Path(__file__).resolve().parents[3]
    report = stage / 'result.json'
    try:
        if sys.platform != 'win32':
            raise ValueError('Runtime provisioning supports Windows x64 only')
        source = Path(__file__).with_name('package-windows.py')
        spec = importlib.util.spec_from_file_location('smart_jobseeker_package', source)
        package = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(package)
        result = install(package, args.tool, stage)
        report.write_text(json.dumps({'ok': True, 'result': result}), encoding='utf-8')
    except Exception as error:
        report.write_text(json.dumps({'ok': False, 'error': str(error)[:2000]}), encoding='utf-8')
        raise SystemExit(1)


if __name__ == '__main__':
    main()
