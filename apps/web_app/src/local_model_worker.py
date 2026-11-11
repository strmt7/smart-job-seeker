"""Bounded model-cache preparation and native private-listener verification.

Standard library only. No model download, inference, credential access or
changes to the source model store. Called through the owned native worker.
"""
from __future__ import annotations

import contextlib
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import socket
import stat
import struct
import sys
import tempfile
import time
import uuid

GIB = 1024**3


def plain(path: Path, directory=False):
    info = path.lstat()
    if stat.S_ISLNK(info.st_mode) or getattr(info, 'st_file_attributes', 0) & 0x400:
        raise ValueError('Model paths must not use reparse points')
    if not (stat.S_ISDIR(info.st_mode) if directory else stat.S_ISREG(info.st_mode)):
        raise ValueError('Model path has the wrong file type')
    return info


def root(path):
    path = Path(path)
    drive = path.drive[4:] if path.drive.startswith('\\\\?\\') else path.drive
    if not path.is_absolute() or '..' in path.parts or (os.name == 'nt' and not re.fullmatch(r'[A-Za-z]:', drive)):
        raise ValueError('Model storage must use an absolute local folder')
    if os.name == 'nt' and not path.drive.startswith('\\\\?\\'):
        path = Path('\\\\?\\' + str(path))
    plain(path, True)
    return path.resolve(strict=True)


def under(base: Path, relative: str):
    parts = relative.split('/')
    if not parts or any(not p or p in ('.', '..') or any(c in p for c in '\\:*?"<>|') for p in parts):
        raise ValueError('Invalid model inventory path')
    path = base
    for index, part in enumerate(parts):
        path /= part
        plain(path, index < len(parts) - 1)
    if not path.resolve(strict=True).is_relative_to(base):
        raise ValueError('Model path escaped its folder')
    return path


def reader(path):
    """Open the final Windows file without following or sharing modifications."""
    if os.name != 'nt':
        return os.fdopen(os.open(path, os.O_RDONLY | getattr(os, 'O_NOFOLLOW', 0)), 'rb')
    import msvcrt
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    create = kernel.CreateFileW
    create.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, ctypes.c_void_p, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
    create.restype = wintypes.HANDLE
    close = kernel.CloseHandle
    close.argtypes = [wintypes.HANDLE]
    close.restype = wintypes.BOOL
    handle = create(str(path), 0x80000000, 1, None, 3, 0x08200000, None)
    if handle == ctypes.c_void_p(-1).value:
        raise OSError('Model file could not be opened safely')
    try:
        descriptor = msvcrt.open_osfhandle(handle, os.O_RDONLY | os.O_BINARY)
    except BaseException:
        if not close(handle):
            raise OSError('Model file handle could not be closed') from None
        raise
    file = os.fdopen(descriptor, 'rb')
    info = os.fstat(file.fileno())
    if not stat.S_ISREG(info.st_mode) or getattr(info, 'st_file_attributes', 0) & 0x400:
        file.close()
        raise ValueError('Model file changed type while opening')
    return file


def bounded(path, limit):
    with reader(path) as stream:
        data = stream.read(limit + 1)
    if not data or len(data) > limit:
        raise ValueError('Model metadata exceeds its bounds')
    return data


def manifest_paths(name):
    if not isinstance(name, str) or len(name) > 256 or not re.fullmatch(r'[A-Za-z0-9_.\-/]+:[A-Za-z0-9_.-]+', name):
        raise ValueError('Use an explicit installed local model and tag')
    name, tag = name.rsplit(':', 1)
    parts = name.split('/')
    if any(not p or p in ('.', '..') for p in [*parts, tag]):
        raise ValueError('Invalid model identity')
    hosts = ['ollama.com', 'registry.ollama.ai']
    if len(parts) > 1 and ('.' in parts[0] or parts[0] == 'localhost'):
        hosts = [parts.pop(0)]
    if len(parts) == 1:
        parts.insert(0, 'library')
    return [str(Path(kind, host, *parts, tag).as_posix())
            for kind in ['manifests-v2', 'manifests'] for host in hosts]


def inventory(store, choice):
    expected = choice.get('digest', '')
    if not re.fullmatch('[a-f0-9]{64}', expected):
        raise ValueError('Invalid selected-model digest')
    for relative in manifest_paths(choice['name']):
        candidate = store / relative
        if candidate.exists():
            raw = bounded(under(store, relative), 1024**2)
            if hashlib.sha256(raw).hexdigest() != expected:
                raise ValueError('The installed model changed after selection')
            break
    else:
        raise ValueError('Selected manifest was not found; check the model storage folder')
    value = json.loads(raw)
    if not isinstance(value, dict) or not isinstance(value.get('layers'), list) or len(value['layers']) > 63:
        raise ValueError('Invalid selected-model manifest')
    files = {relative: {'sha256': expected, 'size': len(raw)}}
    for item in [value.get('config'), *value['layers']]:
        if not isinstance(item, dict) or not re.fullmatch(r'sha256:[a-f0-9]{64}', item.get('digest', '')):
            raise ValueError('Invalid model blob descriptor')
        size = item.get('size')
        if type(size) is not int or not 0 < size <= 20 * GIB:
            raise ValueError('Invalid model blob size')
        path = 'blobs/' + item['digest'].replace(':', '-')
        if plain(under(store, path)).st_size != size:
            raise ValueError('Model blob size differs from its manifest')
        descriptor = {'sha256': item['digest'].split(':')[1], 'size': size}
        if path in files and files[path] != descriptor:
            raise ValueError('Conflicting model blob descriptors')
        files[path] = descriptor
    if len(files) > 65 or sum(f['size'] for f in files.values()) > 20 * GIB:
        raise ValueError('Selected model exceeds the private cache budget')
    return files


def transfer(source, descriptor, deadline, destination=None):
    digest = hashlib.sha256()
    count = 0
    with reader(source) as stream:
        if os.fstat(stream.fileno()).st_size != descriptor['size']:
            raise ValueError('Model file size changed')
        with destination.open('xb') if destination else contextlib.nullcontext() as target:
            while data := stream.read(1024**2):
                count += len(data)
                if count > descriptor['size'] or time.monotonic() > deadline:
                    raise TimeoutError('Model preparation exceeded its byte or time budget')
                digest.update(data)
                if target:
                    target.write(data)
            if target:
                target.flush()
                os.fsync(target.fileno())
    if count != descriptor['size'] or digest.hexdigest() != descriptor['sha256']:
        raise ValueError('Model content does not match its manifest')


@contextlib.contextmanager
def cache_lock(path):
    if os.path.lexists(path):
        plain(path)
    with path.open('a+b') as lock:
        if os.fstat(lock.fileno()).st_size == 0:
            lock.write(b'0')
            lock.flush()
        lock.seek(0)
        if os.name == 'nt':
            import msvcrt
            msvcrt.locking(lock.fileno(), msvcrt.LK_NBLCK, 1)
        else:
            import fcntl
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        yield


def prepare(request):
    choice = request['choice']
    if not isinstance(choice, dict) or set(choice) != {'name', 'digest', 'size_bytes', 'directory'}:
        raise ValueError('Invalid selected model')
    if type(choice['size_bytes']) is not int or not GIB <= choice['size_bytes'] <= 16 * GIB:
        raise ValueError('Invalid selected-model size')
    cache = root(request['cache_root'])
    location = choice['directory'] or os.environ.get('OLLAMA_MODELS') or str(Path.home() / '.ollama/models')
    store = root(location)
    deadline = time.monotonic() + 600
    files = inventory(store, choice)
    final = cache / choice['digest']
    warning = None
    with cache_lock(cache / (choice['digest'] + '.lock')):
        if final.exists():
            plain(final, True)
            record = json.loads(bounded(under(final, 'cache-record.json'), 64 * 1024))
            if record.get('schema') != 1 or record.get('name') != choice['name'] or record.get('digest') != choice['digest']:
                raise ValueError('An unrecognized private cache was preserved')
            try:
                for relative, descriptor in files.items():
                    transfer(under(final / 'models', relative), descriptor, deadline)
                return {'name': choice['name'], 'digest': choice['digest'], 'models': str(final / 'models'), 'reused': True, 'files': files}
            except (OSError, ValueError):
                # Preserve a damaged, recognized cache while replacing it.
                retained = cache / (choice['digest'] + '.replaced-' + uuid.uuid4().hex)
                if final.resolve().parent != cache or retained.resolve().parent != cache or retained.exists():
                    raise ValueError('Private cache repair path changed')
                if len(list(cache.glob(choice['digest'] + '.replaced-*'))) >= 2:
                    raise ValueError('Private cache repair needs disk review; prior copies were preserved')
                warning = retained.name
        needed = sum(item['size'] for item in files.values()) + 2 * GIB
        if shutil.disk_usage(cache).free < needed:
            raise ValueError('Insufficient disk space for a verified model copy and 2 GiB reserve')
        staged = Path(tempfile.mkdtemp(prefix='model-stage-', dir=cache))
        try:
            for relative, descriptor in files.items():
                target = staged / 'models' / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                transfer(under(store, relative), descriptor, deadline, target)
            record = {'schema': 1, 'name': choice['name'], 'digest': choice['digest'], 'files': files}
            (staged / 'cache-record.json').write_text(json.dumps(record), encoding='utf-8')
            if warning:
                os.rename(final, cache / warning)
            if staged.resolve().parent != cache or final.resolve().parent != cache:
                raise ValueError('Private cache destination changed')
            os.rename(staged, final)
        finally:
            if staged.exists():
                if staged.is_symlink() or staged.is_junction() or staged.resolve().parent != cache:
                    raise ValueError('Private staging cleanup path changed')
                shutil.rmtree(staged)
    return {'name': choice['name'], 'digest': choice['digest'], 'models': str(final / 'models'), 'reused': False, 'files': files, 'retained_cache': warning}


def listener(request):
    if os.name != 'nt' or type(request['port']) is not int or not 0 < request['port'] < 65536:
        raise ValueError('Native listener verification requires Windows and a valid port')
    expected = request.get('pid')
    if expected is not None and (type(expected) is not int or not 0 < expected <= 0xffffffff):
        raise ValueError('Invalid expected listener process')
    class Row4(ctypes.Structure):
        _fields_ = [(name, wintypes.DWORD) for name in ['state', 'local', 'port', 'remote', 'remote_port', 'pid']]
    class Row6(ctypes.Structure):
        _fields_ = [('local', ctypes.c_ubyte * 16), ('scope', wintypes.DWORD), ('port', wintypes.DWORD),
                    ('remote', ctypes.c_ubyte * 16), ('remote_scope', wintypes.DWORD),
                    ('remote_port', wintypes.DWORD), ('state', wintypes.DWORD), ('pid', wintypes.DWORD)]
    api = ctypes.WinDLL('iphlpapi', use_last_error=True).GetExtendedTcpTable
    api.argtypes = [ctypes.c_void_p, ctypes.POINTER(wintypes.DWORD), wintypes.BOOL, wintypes.ULONG, ctypes.c_int, wintypes.ULONG]
    api.restype = wintypes.DWORD
    listeners = []
    for family, row_type in [(socket.AF_INET, Row4), (socket.AF_INET6, Row6)]:
        size = wintypes.DWORD()
        if api(None, ctypes.byref(size), False, family, 3, 0) not in (0, 122):
            raise OSError('Listener table could not be sized')
        for _ in range(3):
            if not 4 <= size.value <= 4 * 1024**2:
                raise ValueError('Listener table exceeded its bounds')
            buffer = ctypes.create_string_buffer(size.value)
            code = api(buffer, ctypes.byref(size), False, family, 3, 0)
            if code == 122:
                continue
            if code:
                raise OSError('Listener ownership could not be read')
            count = wintypes.DWORD.from_buffer(buffer).value
            if count > (len(buffer) - 4) // ctypes.sizeof(row_type):
                raise ValueError('Invalid listener row count')
            for index in range(count):
                row = row_type.from_buffer(buffer, 4 + index * ctypes.sizeof(row_type))
                if socket.ntohs(row.port & 0xffff) == request['port']:
                    address = (socket.inet_ntoa(struct.pack('<I', row.local)) if family == socket.AF_INET
                               else socket.inet_ntop(socket.AF_INET6, bytes(row.local)))
                    listeners.append((family, address, row.pid))
            break
        else:
            raise ValueError('Listener table kept changing')
    if expected is None:
        return {'port': request['port'], 'owners': sorted({pid for _, _, pid in listeners})}
    if listeners != [(socket.AF_INET, '127.0.0.1', expected)]:
        raise ValueError('Private listener ownership changed or is ambiguous')
    return {'pid': expected, 'port': request['port']}


def process_state(request):
    """Creation metadata can survive exit; a native wait signal proves exit."""
    if os.name != 'nt' or type(request['pid']) is not int or not 0 < request['pid'] <= 0xffffffff:
        raise ValueError('Native process inspection requires Windows and a valid PID')
    if type(request['created']) is not int or request['created'] <= 0:
        raise ValueError('Native process inspection requires its creation identity')
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    open_process = kernel.OpenProcess
    open_process.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    open_process.restype = wintypes.HANDLE
    times = kernel.GetProcessTimes
    times.argtypes = [wintypes.HANDLE, *([ctypes.POINTER(wintypes.FILETIME)] * 4)]
    times.restype = wintypes.BOOL
    wait = kernel.WaitForSingleObject
    wait.argtypes = [wintypes.HANDLE, wintypes.DWORD]
    wait.restype = wintypes.DWORD
    exit_code = kernel.GetExitCodeProcess
    exit_code.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]
    exit_code.restype = wintypes.BOOL
    close = kernel.CloseHandle
    close.argtypes = [wintypes.HANDLE]
    close.restype = wintypes.BOOL
    handle = open_process(0x1000 | 0x00100000, False, request['pid'])
    if not handle:
        if ctypes.get_last_error() in (87, 1168):
            return {'pid': request['pid'], 'exited': True, 'reason': 'not_present'}
        raise OSError('Native process exit could not be inspected')
    try:
        created, exited, kernel_time, user_time = (wintypes.FILETIME() for _ in range(4))
        if not times(handle, ctypes.byref(created), ctypes.byref(exited), ctypes.byref(kernel_time), ctypes.byref(user_time)):
            raise OSError('Native process creation identity is unavailable')
        token = (created.dwHighDateTime << 32) | created.dwLowDateTime
        if token != request['created']:
            return {'pid': request['pid'], 'exited': True, 'reason': 'pid_reused'}
        state = wait(handle, 0)
        if state == 258:
            return {'pid': request['pid'], 'exited': False, 'reason': 'running'}
        if state != 0:
            raise OSError('Native process wait state is unavailable')
        code = wintypes.DWORD()
        if not exit_code(handle, ctypes.byref(code)):
            raise OSError('Native process exit code is unavailable')
        return {'pid': request['pid'], 'exited': True, 'reason': 'signaled', 'exit_code': code.value}
    finally:
        if not close(handle):
            raise OSError('Native process inspection handle could not be closed')


def main():
    raw = sys.stdin.buffer.read(16 * 1024 + 1)
    if not raw or len(raw) > 16 * 1024:
        raise ValueError('Model worker request exceeds its bounds')
    request = json.loads(raw)
    if not isinstance(request, dict):
        raise ValueError('Invalid model worker request')
    action = request.get('action')
    if action == 'prepare' and set(request) == {'action', 'choice', 'cache_root'}:
        return prepare(request)
    if action == 'listener' and set(request) == {'action', 'port', 'pid'}:
        return listener(request)
    if action == 'endpoint' and set(request) == {'action', 'port'}:
        return listener(request)
    if action == 'process' and set(request) == {'action', 'pid', 'created'}:
        return process_state(request)
    raise ValueError('Unsupported model worker action')


if __name__ == '__main__':
    try:
        print(json.dumps({'ok': True, 'result': main()}))
    except (OSError, ValueError, KeyError, TypeError, TimeoutError) as error:
        message = str(error) if not isinstance(error, OSError) or isinstance(error, TimeoutError) else f'Local model file operation failed (OS error {error.errno}); check the model folder'
        print(json.dumps({'ok': False, 'error': message[:600]}))
        sys.exit(1)
