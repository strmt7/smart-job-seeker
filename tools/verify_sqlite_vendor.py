"""Verify the checksum-bound SQLite binding and its two-line manifest patch."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ARCHIVE_SHA256 = '14a5f01b87b92705d07b433c06f96cbc764637d81a0bffb5734ce17cf0f9a2b3'
FILE_MAP_SHA256 = 'b398bc63bcc2d344f5e70e69abf669f8686007662fdf3c100fcc114247c96045'


def verify(root=ROOT / 'third_party/libsqlite3-sys'):
    if root.is_symlink() or root.is_junction():
        raise ValueError('Unexpected vendored root reparse point')
    metadata = json.loads((root / 'UPSTREAM_PROVENANCE.json').read_text(encoding='utf-8'))
    if (metadata['schema'] != 1 or metadata['package'] != 'libsqlite3-sys' or
            metadata['upstream_version'] != '0.39.1' or
            metadata['local_version'] != '0.39.1+smartjobseeker.1'):
        raise ValueError('Vendored package identity changed')
    files = metadata['upstream_files']
    digest = hashlib.sha256(json.dumps(files, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
    if metadata['archive_sha256'] != ARCHIVE_SHA256 or digest != FILE_MAP_SHA256:
        raise ValueError('Upstream provenance does not match the reviewed archive')
    expected = set(files) | {'UPSTREAM_PROVENANCE.json', 'README.smart-jobseeker.md', 'NOTICE-sqlite.txt'}
    actual = set()
    for path in root.rglob('*'):
        if path.is_symlink() or path.is_junction():
            raise ValueError('Unexpected vendored reparse point')
        if path.is_file():
            actual.add(path.relative_to(root).as_posix())
    if actual != expected:
        raise ValueError('Vendored file inventory changed')
    source = (root / 'sqlite3/sqlite3.c').read_bytes()
    start = source.index(b'/*\n** 2001 September 15')
    end = source.index(b'*/', start) + 2
    if (root / 'NOTICE-sqlite.txt').read_bytes() != source[start:end] + b'\n':
        raise ValueError('SQLite component notice changed')
    for name, expected_hash in files.items():
        data = (root / name).read_bytes()
        if name == 'Cargo.toml':
            # Reverse only the reviewed patch, binding every other manifest byte.
            for patched, original in [
                (b'version = "0.39.1+smartjobseeker.1"', b'version = "0.39.1"'),
                (b'version = "=420.0.0"', b'version = "=419.0.6"'),
            ]:
                if data.count(patched) != 1:
                    raise ValueError('Vendored manifest patch changed')
                data = data.replace(patched, original)
        if hashlib.sha256(data).hexdigest() != expected_hash:
            raise ValueError('Vendored upstream bytes changed: ' + name)
    return {'upstream_files': len(files), 'patched_files': ['Cargo.toml'], 'archive_sha256': ARCHIVE_SHA256}


if __name__ == '__main__':
    print(json.dumps(verify()))
