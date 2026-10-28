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
import tomllib
import zipfile

REPO = Path(__file__).resolve().parents[3]
TARGET = 'x86_64-pc-windows-msvc'

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', required=True, help='New ZIP path inside this repository')
    parser.add_argument('--release', action='store_true')
    parser.add_argument('--openssl-root', default=os.environ.get('OPENSSL_DIR') or str(Path(os.environ.get('ProgramFiles', 'C:/Program Files')) / 'OpenSSL-Win64'))
    args = parser.parse_args()
    output = Path(args.output).resolve()
    if not output.is_relative_to(REPO) or output.suffix.lower() != '.zip' or output.exists():
        raise ValueError('Output must be a new .zip path inside the repository')
    profile = 'release' if args.release else 'debug'
    build = REPO / 'target' / profile
    exe = build / 'waypoint_web.exe'
    dlls = list(build.glob('libcrypto*.dll'))
    if not exe.is_file() or len(dlls) != 1:
        raise ValueError('Build Waypoint first and ensure exactly one libcrypto DLL is next to the executable')
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
    source_dll = Path(args.openssl_root) / 'bin' / dlls[0].name
    if not source_dll.is_file() or digest(source_dll) != digest(dlls[0]):
        raise ValueError('OpenSSL runtime does not match the specified license source')
    openssl_license = Path(args.openssl_root) / 'license.txt'
    if not openssl_license.is_file():
        raise ValueError('OpenSSL license.txt is missing from the specified installation')
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
        if package.get('license-file'):
            license_file = (root / package['license-file']).resolve()
            if not license_file.is_relative_to(root.resolve()) or not license_file.is_file(): raise ValueError(f'Invalid license file for {name} {version}')
            files.add(license_file)
        if not files: raise ValueError(f'No local license text for {name} {version}')
        packages[(name,version)] = (package.get('license'), sorted(files))
    output.parent.mkdir(parents=True,exist_ok=True)
    # Exclusive creation avoids replacing an existing artifact; remove a partial ZIP on failure.
    created = False
    try:
        raw = output.open('xb')
        created = True
        with raw, zipfile.ZipFile(raw,'w',zipfile.ZIP_DEFLATED,strict_timestamps=False) as archive:
            def add(path, dest): archive.write(path, dest)
            add(exe,'app/waypoint_web.exe');add(dlls[0],f'app/{dlls[0].name}')
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
            archive.writestr('app/package-manifest.json',json.dumps({'profile':profile,'executable_sha256':hashlib.sha256(exe.read_bytes()).hexdigest(),'openssl_dll':dlls[0].name,'openssl_sha256':hashlib.sha256(dlls[0].read_bytes()).hexdigest(),'note':'Prototype bundle. No applications are sent by the interface.'},indent=2))
            archive.writestr('INSTALL.txt','Extract the entire archive. On Windows, run install.ps1 in PowerShell for a per-user copy, or launch app/waypoint_web.exe directly. Microsoft Visual C++ runtime, a Chromium browser, Ollama, and optional document tools may need separate installation. See app/README.md.\n')
    except BaseException:
        if created and output.is_file(): output.unlink()
        raise
    print(f'Created {output} with {len(packages)} dependency license inventories.')

if __name__ == '__main__':
    try: main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f'Packaging failed: {error}',file=sys.stderr);sys.exit(1)
