"""Generate the embedded Rust license inventory from the locked Cargo graph."""
import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path
import subprocess
import sys

REPO = Path(__file__).resolve().parents[3]
DESTINATION = REPO / 'docs' / 'dependency-license-inventory.txt'


def inventory(metadata, lock_bytes):
    packages = metadata['packages']
    resolved = {node['id'] for node in metadata['resolve']['nodes']}
    by_id = {package['id']: package for package in packages}
    if len(by_id) != len(packages) or set(by_id) != resolved:
        raise ValueError('Expected complete, uniquely identified resolved package metadata')
    groups = defaultdict(list)
    for package in packages:
        expression = package.get('license')
        if not isinstance(expression, str) or not expression.strip():
            raise ValueError('Missing license expression: ' + package['name'])
        if any(character in expression for character in '\r\n\x00'):
            raise ValueError('Invalid multiline license expression')
        groups[expression].append(package['name'] + '@' + package['version'])
    workspace = set(metadata['workspace_members'])
    if not workspace.issubset(resolved):
        raise ValueError('Workspace member is absent from the resolved graph')
    lines = [
        'Rust dependency license inventory',
        'Scope: cargo metadata --locked --all-features; all resolved target platforms.',
        'Cargo.lock SHA-256 (LF-normalized): '
        + hashlib.sha256(lock_bytes.replace(b'\r\n', b'\n')).hexdigest(),
        f'Packages: {len(packages)} total; {len(workspace)} workspace; '
        f'{len(packages) - len(workspace)} external.',
        'License expressions are preserved verbatim, including OR alternatives and AND obligations.',
        'This inventory is not the full license text or a declaration that every alternative is permitted.',
        'Distribution bundles collect dependency license files separately; cargo-deny enforces policy.',
        '',
    ]
    for expression, names in sorted(groups.items()):
        lines.append(f'{expression} ({len(names)}): ' + ', '.join(sorted(names)))
    return '\n'.join(lines) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='Fail if the saved inventory is stale')
    args = parser.parse_args()
    lock_path = REPO / 'Cargo.lock'
    lock_bytes = lock_path.read_bytes()
    result = subprocess.run(
        ['cargo', 'metadata', '--locked', '--all-features', '--format-version', '1'],
        cwd=REPO, capture_output=True, timeout=120, check=True,
    )
    if len(result.stdout) > 16 * 1024 * 1024:
        raise ValueError('Cargo metadata exceeds the inventory size budget')
    if lock_path.read_bytes() != lock_bytes:
        raise ValueError('Cargo.lock changed while generating the inventory; retry after it stabilizes')
    text = inventory(json.loads(result.stdout), lock_bytes)
    if args.check:
        if not DESTINATION.is_file() or DESTINATION.read_text(encoding='utf-8') != text:
            raise ValueError('License inventory is stale; run this script without --check and review the diff')
        print('Locked Rust license inventory is current.')
    else:
        DESTINATION.write_text(text, encoding='utf-8', newline='\n')
        print('Updated docs/dependency-license-inventory.txt from the locked Cargo graph.')


if __name__ == '__main__':
    try:
        main()
    except (ValueError, KeyError, OSError, subprocess.SubprocessError) as error:
        print(f'License inventory failed: {error}', file=sys.stderr)
        sys.exit(1)
