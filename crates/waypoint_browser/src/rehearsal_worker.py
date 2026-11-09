"""Windows-only AppContainer browser worker. Standard library; no network broker.

The first stdin line is a checksum-bound engine/content specification. Later
lines are CDP commands. Browser traffic stays inside a zero-capability token;
only its debugging pipe crosses the boundary. This is experimental until the
complete rehearsal boundary has been qualified. Never use it on real forms yet.
"""
from __future__ import annotations

import ctypes as ct
from ctypes import wintypes as wt
import hashlib
import json
import math
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import sys
import time
import uuid

MAX_FRAME = 16 * 1024 * 1024
MAX_CONFIG = 1024 * 1024
PREFIX = 'SmartJobseeker.Rehearsal.'
CONTENT_SUFFIXES = {'.html', '.htm', '.js', '.mjs', '.css', '.json', '.png', '.jpg',
                    '.jpeg', '.svg', '.gif', '.webp', '.ico', '.woff', '.woff2',
                    '.ttf', '.otf', '.pdf', '.webmanifest', '.txt'}


class WorkerError(RuntimeError):
    """Messages are fixed diagnostics, never browser payloads or applicant text."""


class RecoveryBusy(WorkerError):
    """An original owned process is still active; do not admit another job."""


def relative_path(value):
    if not isinstance(value, str) or not value or len(value) > 240:
        raise WorkerError('invalid_relative_path')
    parts = value.split('/')
    if any(not part or part in ('.', '..') or part.endswith((' ', '.')) or
           re.search(r'[\\:*?"<>|\x00-\x1f]', part) or
           re.fullmatch(r'(?i)(con|prn|aux|nul|com[1-9¹²³]|lpt[1-9¹²³])(?:\..*)?', part)
           for part in parts):
        raise WorkerError('invalid_relative_path')
    return PurePosixPath(value)


def parse_json(raw, limit):
    if not raw or len(raw) > limit:
        raise WorkerError('protocol_size_limit')
    try:
        def invalid(_):
            raise ValueError('nonfinite')
        def finite(raw):
            value = float(raw)
            if not math.isfinite(value):
                raise ValueError('nonfinite')
            return value
        def unique(pairs):
            result = {}
            for key, value in pairs:
                if key in result:
                    raise ValueError('duplicate key')
                result[key] = value
            return result
        # json.loads(bytes) also accepts UTF-16/32. The pipe contract is UTF-8
        # only, so decode explicitly instead of accepting ambiguous encodings.
        decoded = raw.decode('utf-8', errors='strict') if isinstance(raw, bytes) else raw
        return json.loads(decoded, parse_constant=invalid, parse_float=finite, object_pairs_hook=unique)
    except (ValueError, UnicodeError, RecursionError):
        raise WorkerError('invalid_protocol_json') from None


def validate_command(value):
    if value == {'op': 'close'}:
        return False
    if (not isinstance(value, dict) or not {'id', 'method', 'params'} <= value.keys()
            or value.keys() - {'id', 'method', 'params', 'sessionId'}
            or type(value['id']) is not int or not 0 < value['id'] < 2**53
            or not isinstance(value['method'], str)
            or len(value['method']) > 128
            or not re.fullmatch(r'[A-Za-z][A-Za-z0-9]*\.[A-Za-z][A-Za-z0-9]*', value['method'])
            or not isinstance(value['params'], dict)):
        raise WorkerError('invalid_cdp_command')
    session = value.get('sessionId')
    if 'sessionId' in value and (not isinstance(session, str) or not session or
                                 len(session) > 512 or any(ord(c) < 32 for c in session)):
        raise WorkerError('invalid_cdp_session')
    return True


def manifest(value):
    if not isinstance(value, dict) or not value or len(value) > 4096:
        raise WorkerError('invalid_file_manifest')
    seen = set()
    for name, digest in value.items():
        relative_path(name)
        folded = name.casefold()
        if folded in seen or not isinstance(digest, str) or not re.fullmatch('[a-f0-9]{64}', digest):
            raise WorkerError('invalid_file_manifest')
        seen.add(folded)
    return value


def source_root(value):
    if not isinstance(value, str) or not Path(value).is_absolute():
        raise WorkerError('source_root_must_be_absolute')
    root = Path(value)
    if root.is_symlink() or getattr(root, 'is_junction', lambda: False)() or not root.is_dir():
        raise WorkerError('invalid_source_root')
    return root.resolve(strict=True)


def copy_verified(root, files, destination, budget, content=False):
    files = manifest(files)
    actual = set()
    total = 0
    for path in root.rglob('*'):
        if path.is_symlink() or getattr(path, 'is_junction', lambda: False)():
            raise WorkerError('source_reparse_point')
        if path.is_file():
            name = path.relative_to(root).as_posix()
            relative_path(name)
            actual.add(name)
            total += path.stat().st_size
            if content and path.suffix.lower() not in CONTENT_SUFFIXES:
                raise WorkerError('unsupported_snapshot_file')
            if len(actual) > 4096 or total > budget:
                raise WorkerError('source_copy_budget_exceeded')
    if actual != set(files) or shutil.disk_usage(destination.parent).free < total + 1024**3:
        raise WorkerError('source_inventory_or_disk_budget_mismatch')
    destination.mkdir()
    copied = 0
    for name in sorted(files):
        source = root.joinpath(*relative_path(name).parts)
        target = destination.joinpath(*relative_path(name).parts)
        target.parent.mkdir(parents=True, exist_ok=True)
        digest = hashlib.sha256()
        with source.open('rb') as incoming, target.open('xb') as outgoing:
            if not stat.S_ISREG(os.fstat(incoming.fileno()).st_mode):
                raise WorkerError('source_is_not_regular_file')
            while chunk := incoming.read(64 * 1024):
                copied += len(chunk)
                if copied > budget:
                    raise WorkerError('source_copy_budget_exceeded')
                digest.update(chunk)
                outgoing.write(chunk)
        if digest.hexdigest() != files[name]:
            raise WorkerError('source_hash_mismatch')
    return copied


P = ct.c_void_p
SIZE = ct.c_size_t


class Startup(ct.Structure):
    _fields_ = [('cb', wt.DWORD), ('reserved', wt.LPWSTR), ('desktop', wt.LPWSTR),
                ('title', wt.LPWSTR), ('x', wt.DWORD), ('y', wt.DWORD), ('xs', wt.DWORD),
                ('ys', wt.DWORD), ('xc', wt.DWORD), ('yc', wt.DWORD), ('fill', wt.DWORD),
                ('flags', wt.DWORD), ('show', wt.WORD), ('reserved2', wt.WORD),
                ('reservedPtr', P), ('stdin', wt.HANDLE), ('stdout', wt.HANDLE), ('stderr', wt.HANDLE)]


class Extended(ct.Structure):
    _fields_ = [('startup', Startup), ('attributes', P)]


class Process(ct.Structure):
    _fields_ = [('process', wt.HANDLE), ('thread', wt.HANDLE), ('pid', wt.DWORD), ('tid', wt.DWORD)]


class Capabilities(ct.Structure):
    _fields_ = [('sid', P), ('capabilities', P), ('count', wt.DWORD), ('reserved', wt.DWORD)]


class SecurityAttributes(ct.Structure):
    _fields_ = [('length', wt.DWORD), ('descriptor', P), ('inherit', wt.BOOL)]


class BasicLimits(ct.Structure):
    _fields_ = [('processTime', ct.c_longlong), ('jobTime', ct.c_longlong), ('flags', wt.DWORD),
                ('minWorking', SIZE), ('maxWorking', SIZE), ('processLimit', wt.DWORD),
                ('affinity', SIZE), ('priority', wt.DWORD), ('scheduling', wt.DWORD)]


class IoCounters(ct.Structure):
    _fields_ = [(name, ct.c_ulonglong) for name in
                ['readOps', 'writeOps', 'otherOps', 'readBytes', 'writeBytes', 'otherBytes']]


class Limits(ct.Structure):
    _fields_ = [('basic', BasicLimits), ('io', IoCounters), ('processMemory', SIZE),
                ('jobMemory', SIZE), ('peakProcess', SIZE), ('peakJob', SIZE)]


class Accounting(ct.Structure):
    _fields_ = [('userTime', ct.c_longlong), ('kernelTime', ct.c_longlong),
                ('periodUser', ct.c_longlong), ('periodKernel', ct.c_longlong),
                ('faults', wt.DWORD), ('total', wt.DWORD), ('active', wt.DWORD), ('terminated', wt.DWORD)]


class MemoryStatus(ct.Structure):
    _fields_ = [('length', wt.DWORD), ('load', wt.DWORD)] + [(name, ct.c_ulonglong) for name in
        ['total_physical', 'available_physical', 'total_page', 'available_page',
         'total_virtual', 'available_virtual', 'available_extended']]


class Windows:
    def __init__(self):
        if sys.platform != 'win32' or ct.sizeof(P) != 8:
            raise WorkerError('requires_64bit_windows')
        import msvcrt
        self.msvcrt = msvcrt
        self.kernel = ct.WinDLL('kernel32', use_last_error=True)
        self.userenv = ct.WinDLL('userenv', use_last_error=True)
        self.advapi = ct.WinDLL('advapi32', use_last_error=True)
        self.ole = ct.WinDLL('ole32', use_last_error=True)
        declarations = [
            ('userenv', 'CreateAppContainerProfile', ct.c_long, [wt.LPCWSTR, wt.LPCWSTR, wt.LPCWSTR, P, wt.DWORD, ct.POINTER(P)]),
            ('userenv', 'DeleteAppContainerProfile', ct.c_long, [wt.LPCWSTR]),
            ('userenv', 'GetAppContainerFolderPath', ct.c_long, [wt.LPCWSTR, ct.POINTER(P)]),
            ('advapi', 'ConvertSidToStringSidW', wt.BOOL, [P, ct.POINTER(P)]),
            ('advapi', 'ConvertStringSecurityDescriptorToSecurityDescriptorW', wt.BOOL, [wt.LPCWSTR, wt.DWORD, ct.POINTER(P), P]),
            ('advapi', 'FreeSid', P, [P]),
            ('advapi', 'OpenProcessToken', wt.BOOL, [wt.HANDLE, wt.DWORD, ct.POINTER(wt.HANDLE)]),
            ('advapi', 'GetTokenInformation', wt.BOOL, [wt.HANDLE, wt.DWORD, P, wt.DWORD, ct.POINTER(wt.DWORD)]),
            ('advapi', 'EqualSid', wt.BOOL, [P, P]),
            ('advapi', 'GetSecurityDescriptorDacl', wt.BOOL, [P, ct.POINTER(wt.BOOL), ct.POINTER(P), ct.POINTER(wt.BOOL)]),
            ('advapi', 'SetNamedSecurityInfoW', wt.DWORD, [wt.LPWSTR, ct.c_int, wt.DWORD, P, P, P, P]),
            ('kernel', 'InitializeProcThreadAttributeList', wt.BOOL, [P, wt.DWORD, wt.DWORD, ct.POINTER(SIZE)]),
            ('kernel', 'UpdateProcThreadAttribute', wt.BOOL, [P, wt.DWORD, SIZE, P, SIZE, P, P]),
            ('kernel', 'DeleteProcThreadAttributeList', None, [P]),
            ('kernel', 'CreateProcessW', wt.BOOL, [wt.LPCWSTR, wt.LPWSTR, P, P, wt.BOOL, wt.DWORD, P, wt.LPCWSTR, P, ct.POINTER(Process)]),
            ('kernel', 'CreatePipe', wt.BOOL, [ct.POINTER(wt.HANDLE), ct.POINTER(wt.HANDLE), ct.POINTER(SecurityAttributes), wt.DWORD]),
            ('kernel', 'PeekNamedPipe', wt.BOOL, [wt.HANDLE, P, wt.DWORD, P, ct.POINTER(wt.DWORD), P]),
            ('kernel', 'SetHandleInformation', wt.BOOL, [wt.HANDLE, wt.DWORD, wt.DWORD]),
            ('kernel', 'CloseHandle', wt.BOOL, [wt.HANDLE]),
            ('kernel', 'ResumeThread', wt.DWORD, [wt.HANDLE]),
            ('kernel', 'WaitForSingleObject', wt.DWORD, [wt.HANDLE, wt.DWORD]),
            ('kernel', 'TerminateProcess', wt.BOOL, [wt.HANDLE, wt.UINT]),
            ('kernel', 'CreateJobObjectW', wt.HANDLE, [P, wt.LPCWSTR]),
            ('kernel', 'OpenJobObjectW', wt.HANDLE, [wt.DWORD, wt.BOOL, wt.LPCWSTR]),
            ('kernel', 'SetInformationJobObject', wt.BOOL, [wt.HANDLE, ct.c_int, P, wt.DWORD]),
            ('kernel', 'QueryInformationJobObject', wt.BOOL, [wt.HANDLE, ct.c_int, P, wt.DWORD, P]),
            ('kernel', 'TerminateJobObject', wt.BOOL, [wt.HANDLE, wt.UINT]),
            ('kernel', 'OpenProcess', wt.HANDLE, [wt.DWORD, wt.BOOL, wt.DWORD]),
            ('kernel', 'GetCurrentProcess', wt.HANDLE, []),
            ('kernel', 'GetProcessId', wt.DWORD, [wt.HANDLE]),
            ('kernel', 'GetProcessTimes', wt.BOOL, [wt.HANDLE, ct.POINTER(wt.FILETIME), ct.POINTER(wt.FILETIME), ct.POINTER(wt.FILETIME), ct.POINTER(wt.FILETIME)]),
            ('kernel', 'CreateDirectoryW', wt.BOOL, [wt.LPCWSTR, ct.POINTER(SecurityAttributes)]),
            ('kernel', 'MoveFileExW', wt.BOOL, [wt.LPCWSTR, wt.LPCWSTR, wt.DWORD]),
            ('kernel', 'IsProcessInJob', wt.BOOL, [wt.HANDLE, wt.HANDLE, ct.POINTER(wt.BOOL)]),
            ('kernel', 'LocalFree', P, [P]),
            ('kernel', 'GlobalMemoryStatusEx', wt.BOOL, [ct.POINTER(MemoryStatus)]),
            ('kernel', 'CreateMutexW', wt.HANDLE, [P, wt.BOOL, wt.LPCWSTR]),
            ('kernel', 'ReleaseMutex', wt.BOOL, [wt.HANDLE]),
            ('ole', 'CoTaskMemFree', None, [P]),
        ]
        for library, name, result, arguments in declarations:
            function = getattr(getattr(self, library), name)
            function.restype, function.argtypes = result, arguments
            setattr(self, name, function)
        token = wt.HANDLE()
        self.check(self.OpenProcessToken(self.GetCurrentProcess(), 8, ct.byref(token)), 'user_token_open_failed')
        try:
            used = wt.DWORD()
            self.GetTokenInformation(token, 1, None, 0, ct.byref(used))
            if not ct.sizeof(P) <= used.value <= 65536:
                raise WorkerError('user_token_size_invalid')
            buffer = ct.create_string_buffer(used.value)
            self.check(self.GetTokenInformation(token, 1, buffer, used, ct.byref(used)), 'user_token_query_failed')
            self.user_sid = self.sid_text(ct.cast(buffer, ct.POINTER(P)).contents.value)
        finally:
            self.CloseHandle(token)

    @staticmethod
    def check(value, diagnostic):
        if not value:
            raise WorkerError(diagnostic + ':' + str(ct.get_last_error()))
        return value

    def token(self, process, sid):
        token = wt.HANDLE()
        self.check(self.OpenProcessToken(process, 8, ct.byref(token)), 'token_open_failed')
        try:
            used, container = wt.DWORD(), wt.DWORD()
            self.check(self.GetTokenInformation(token, 29, ct.byref(container), ct.sizeof(container), ct.byref(used)), 'token_query_failed')
            if container.value != 1:
                raise WorkerError('token_is_not_appcontainer')
            for kind in [30, 31]:
                self.GetTokenInformation(token, kind, None, 0, ct.byref(used))
                if not 4 <= used.value <= 65536:
                    raise WorkerError('token_information_size_invalid')
                buffer = ct.create_string_buffer(used.value)
                self.check(self.GetTokenInformation(token, kind, buffer, used, ct.byref(used)), 'token_query_failed')
                if kind == 30 and ct.cast(buffer, ct.POINTER(wt.DWORD)).contents.value != 0:
                    raise WorkerError('token_has_capabilities')
                if kind == 31 and not self.EqualSid(ct.cast(buffer, ct.POINTER(P)).contents.value, sid):
                    raise WorkerError('token_appcontainer_identity_changed')
        finally:
            self.CloseHandle(token)

    def sid_text(self, sid):
        raw = P()
        self.check(self.ConvertSidToStringSidW(sid, ct.byref(raw)), 'sid_string_failed')
        try:
            return ct.wstring_at(raw)
        finally:
            self.LocalFree(raw)

    def identity(self, process):
        created, exited, kernel, user = (wt.FILETIME() for _ in range(4))
        self.check(self.GetProcessTimes(process, ct.byref(created), ct.byref(exited), ct.byref(kernel), ct.byref(user)), 'process_time_query_failed')
        pid = self.GetProcessId(process)
        self.check(pid, 'process_identity_failed')
        return {'pid': pid, 'created': str((created.dwHighDateTime << 32) | created.dwLowDateTime)}

    def identity_alive(self, identity):
        handle = self.OpenProcess(0x100000 | 0x1000, False, identity['pid'])
        if not handle:
            if ct.get_last_error() == 87:  # ERROR_INVALID_PARAMETER: no such PID.
                return False
            raise WorkerError('recovery_process_identity_unavailable')
        try:
            if self.identity(handle) != identity:
                return False  # Reused PID; never touch its new owner.
            result = self.WaitForSingleObject(handle, 0)
            if result not in (0, 258):
                raise WorkerError('recovery_process_wait_failed')
            return result == 258
        finally:
            self.CloseHandle(handle)

    def acquire_admission(self):
        handle = self.CreateMutexW(None, False, 'Global\\SmartJobseeker.RehearsalAdmission.' + self.user_sid)
        self.check(handle, 'rehearsal_admission_failed')
        # An abandoned mutex is acquired normally after its owning thread dies.
        result = self.WaitForSingleObject(handle, 0)
        if result not in (0, 0x80):
            self.CloseHandle(handle)
            raise WorkerError('another_rehearsal_is_active' if result == 258 else 'rehearsal_admission_failed')
        return handle

    def available_ram(self):
        status = MemoryStatus()
        status.length = ct.sizeof(status)
        self.check(self.GlobalMemoryStatusEx(ct.byref(status)), 'memory_inspection_failed')
        return status.available_physical


def profile_id(name):
    if not isinstance(name, str) or not re.fullmatch(re.escape(PREFIX) + '[a-f0-9]{32}', name):
        raise WorkerError('invalid_recovery_profile')
    return name[len(PREFIX):]


def job_name(user_sid, name):
    return 'Global\\SmartJobseeker.RehearsalJob.' + user_sid + '.' + profile_id(name)


def validate_identity(value):
    if (not isinstance(value, dict) or set(value) != {'pid', 'created'} or
            type(value['pid']) is not int or not 0 < value['pid'] <= 0xffffffff or
            not isinstance(value['created'], str) or not re.fullmatch('[1-9][0-9]{0,19}', value['created']) or
            int(value['created']) > 0xffffffffffffffff):
        raise WorkerError('invalid_recovery_identity')
    return value


def validate_receipt(value, user_sid):
    keys = {'schema', 'mechanism', 'profile', 'user_sid', 'worker', 'browser', 'job'}
    if (not isinstance(value, dict) or set(value) != keys or type(value['schema']) is not int or
            value['schema'] != 1 or value['mechanism'] != 'windows-job-list-v1' or
            value['user_sid'] != user_sid):
        raise WorkerError('invalid_recovery_receipt')
    profile_id(value['profile'])
    validate_identity(value['worker'])
    if value['browser'] is not None:
        validate_identity(value['browser'])
    if value['job'] != job_name(user_sid, value['profile']):
        raise WorkerError('invalid_recovery_job')
    return value


def reject_reparse(path):
    if path.is_symlink() or getattr(path.lstat(), 'st_file_attributes', 0) & 0x400:
        raise WorkerError('recovery_reparse_point')


def bounded_entries(root):
    result = []
    for entry in root.iterdir():
        if len(result) >= 256:
            raise WorkerError('recovery_inventory_budget_exceeded')
        result.append(entry)
    return result


def profile_usage(root, max_entries=10000, max_bytes=1024**3):
    """Bounded metadata-only walk; never follow a reparse point."""
    pending = [root]
    count = total = 0
    while pending:
        folder = pending.pop()
        try:
            reject_reparse(folder)
            with os.scandir(folder) as entries:
                for entry in entries:
                    count += 1
                    if count > max_entries:
                        raise WorkerError('profile_storage_entry_budget_exceeded')
                    try:
                        info = entry.stat(follow_symlinks=False)
                    except FileNotFoundError:
                        continue  # Browser temporary files may disappear.
                    if stat.S_ISLNK(info.st_mode) or getattr(info, 'st_file_attributes', 0) & 0x400:
                        raise WorkerError('profile_storage_reparse_point')
                    if stat.S_ISDIR(info.st_mode):
                        pending.append(Path(entry.path))
                    elif stat.S_ISREG(info.st_mode):
                        total += info.st_size
                        if total > max_bytes:
                            raise WorkerError('profile_storage_byte_budget_exceeded')
                    else:
                        raise WorkerError('unsupported_profile_storage_entry')
        except FileNotFoundError:
            if folder == root:
                raise WorkerError('profile_storage_unavailable') from None
    return total


class RecoveryState:
    """Private write-ahead ownership records. Never store applicant data here.

    Every caller must hold the per-user global admission mutex. Recovery can
    query old jobs/processes, but has no API that terminates a process by PID.
    """
    def __init__(self, api):
        self.api = api
        local = Path(os.environ['LOCALAPPDATA'])
        if not local.is_absolute() or not local.is_dir():
            raise WorkerError('recovery_local_appdata_unavailable')
        self.local = local.resolve(strict=True)
        parent = self.local / 'SmartJobseeker'
        parent.mkdir(exist_ok=True)
        reject_reparse(parent)
        self.root = parent / 'rehearsal-state-v1'
        descriptor = P()
        api.check(api.ConvertStringSecurityDescriptorToSecurityDescriptorW(
            'D:P(A;OICI;FA;;;' + api.user_sid + ')(A;OICI;FA;;;SY)',
            1, ct.byref(descriptor), None), 'recovery_security_descriptor_failed')
        try:
            security = SecurityAttributes(ct.sizeof(SecurityAttributes), descriptor, False)
            if not api.CreateDirectoryW(str(self.root), ct.byref(security)) and ct.get_last_error() != 183:
                raise WorkerError('recovery_directory_create_failed')
            reject_reparse(self.root)
            present, defaulted, dacl = wt.BOOL(), wt.BOOL(), P()
            api.check(api.GetSecurityDescriptorDacl(descriptor, ct.byref(present), ct.byref(dacl), ct.byref(defaulted)), 'recovery_dacl_failed')
            if not present.value or not dacl:
                raise WorkerError('recovery_dacl_missing')
            # Protect this one application-owned directory, not the surrounding
            # user profile or unrelated application's data.
            if api.SetNamedSecurityInfoW(str(self.root), 1, 4 | 0x80000000, None, None, dacl, None):
                raise WorkerError('recovery_directory_protection_failed')
        finally:
            api.LocalFree(descriptor)

    def path(self, name):
        return self.root / (profile_id(name) + '.json')

    def save(self, value):
        validate_receipt(value, self.api.user_sid)
        if len(bounded_entries(self.root)) >= 256:
            raise WorkerError('recovery_inventory_budget_exceeded')
        path = self.path(value['profile'])
        if path.exists():
            reject_reparse(path)
        temporary = self.root / (profile_id(value['profile']) + '.' + uuid.uuid4().hex + '.tmp')
        raw = json.dumps(value, separators=(',', ':')).encode('utf-8')
        if len(raw) > 4096:
            raise WorkerError('recovery_record_budget_exceeded')
        with temporary.open('xb') as stream:
            stream.write(raw)
            stream.flush()
            os.fsync(stream.fileno())
        self.api.check(self.api.MoveFileExW(str(temporary), str(path), 1 | 8), 'recovery_record_commit_failed')

    def forget(self, name):
        path = self.path(name)
        if path.exists():
            reject_reparse(path)
            path.unlink()

    def job_active(self, name):
        handle = self.api.OpenJobObjectW(4, False, name)  # JOB_OBJECT_QUERY only.
        if not handle:
            if ct.get_last_error() == 2:
                return False
            raise WorkerError('recovery_job_unavailable')
        try:
            accounting = Accounting()
            self.api.check(self.api.QueryInformationJobObject(handle, 1, ct.byref(accounting), ct.sizeof(accounting), None), 'recovery_job_query_failed')
            return accounting.active != 0
        finally:
            self.api.CloseHandle(handle)

    def remove_profile(self, name):
        # Derive the only permitted location; receipt data never supplies paths.
        package = self.local / 'Packages' / name
        if package.exists():
            reject_reparse(package)
            scanned = 0
            for path in package.rglob('*'):
                reject_reparse(path)
                scanned += 1
                if scanned > 10000:
                    raise WorkerError('recovery_profile_inventory_budget_exceeded')
        result = self.api.DeleteAppContainerProfile(name)
        # ERROR_FILE_NOT_FOUND is an idempotent result for an uncreated profile.
        if result < 0 and (result & 0xffffffff) != 0x80070002:
            raise WorkerError('recovery_profile_delete_failed')
        if (package / 'AC').exists():
            raise WorkerError('recovery_profile_files_retained')

    def recover(self):
        entries = bounded_entries(self.root)
        recovered = retained = 0
        deadline = time.monotonic() + 10
        for path in sorted(entries):
            if time.monotonic() >= deadline:
                raise WorkerError('recovery_time_budget_exceeded')
            try:
                reject_reparse(path)
                if re.fullmatch('[a-f0-9]{32}\\.[a-f0-9]{32}\\.tmp', path.name) and path.is_file():
                    # Uncommitted staging files cannot precede profile creation.
                    # With the admission mutex held no other writer is active.
                    path.unlink()
                    continue
                if not re.fullmatch('[a-f0-9]{32}\\.json', path.name) or not path.is_file():
                    raise WorkerError('unknown_recovery_entry')
                with path.open('rb') as stream:
                    value = validate_receipt(parse_json(stream.read(4097), 4096), self.api.user_sid)
                if path != self.path(value['profile']):
                    raise WorkerError('recovery_record_name_mismatch')
                if self.api.identity_alive(value['worker']):
                    raise RecoveryBusy('recovery_worker_still_active')
                wait_until = min(deadline, time.monotonic() + 3)
                while self.job_active(value['job']) or (value['browser'] is not None and self.api.identity_alive(value['browser'])):
                    if time.monotonic() >= wait_until:
                        raise RecoveryBusy('recovery_browser_still_active')
                    time.sleep(.02)
                self.remove_profile(value['profile'])
                self.forget(value['profile'])
                recovered += 1
            except RecoveryBusy:
                raise
            except (WorkerError, OSError, ValueError):
                # Unknown/corrupt ownership never authorizes deletion. Surface
                # the pending denominator without blocking independent work.
                retained += 1
        return {'recovered': recovered, 'retained': retained}


class Container:
    def __init__(self, api):
        self.api, self.name = api, PREFIX + uuid.uuid4().hex
        self.sid, self.folder, self.job = P(), None, None
        self.process = Process()
        self.fds, self.child_fds = [], []
        self.read_fd = self.write_fd = None
        self.created = False
        self.admission = None
        self.checked_pids = set()
        self.state = None
        self.receipt = None
        self.recovery = {'recovered': 0, 'retained': 0}
        self.peak_commit = None

    def prepare(self, config):
        api = self.api
        self.admission = api.acquire_admission()
        self.state = RecoveryState(api)
        self.recovery = self.state.recover()
        if api.available_ram() < 6 * 1024**3:
            raise WorkerError('rehearsal_requires_6_gib_available_ram')
        self.receipt = {'schema': 1, 'mechanism': 'windows-job-list-v1', 'profile': self.name,
                        'user_sid': api.user_sid, 'worker': api.identity(api.GetCurrentProcess()),
                        'browser': None, 'job': job_name(api.user_sid, self.name)}
        self.state.save(self.receipt)
        if api.CreateAppContainerProfile(self.name, 'Smart Jobseeker rehearsal', 'Private offline browser rehearsal', None, 0, ct.byref(self.sid)) < 0:
            self.state.forget(self.name)
            self.receipt = None
            raise WorkerError('profile_create_failed')
        self.created = True
        raw = P()
        api.check(api.ConvertSidToStringSidW(self.sid, ct.byref(raw)), 'profile_sid_failed')
        try:
            sid_text = ct.wstring_at(raw)
        finally:
            api.LocalFree(raw)
        location = P()
        if api.GetAppContainerFolderPath(sid_text, ct.byref(location)) < 0:
            raise WorkerError('profile_folder_failed')
        try:
            self.folder = Path(ct.wstring_at(location)).resolve()
        finally:
            api.CoTaskMemFree(location)
        expected = (Path(os.environ['LOCALAPPDATA']) / 'Packages' / self.name / 'AC').resolve()
        if self.folder != expected:
            raise WorkerError('profile_folder_mismatch')
        engine = self.folder / 'engine'
        copy_verified(source_root(config['engine_root']), config['engine_files'], engine, 600 * 1024**2)
        binary = engine / 'chrome-headless-shell.exe'
        if not binary.is_file() or not (engine / 'icudtl.dat').is_file():
            raise WorkerError('headless_engine_incomplete')
        content = self.folder / 'content'
        copy_verified(source_root(config['content_root']), config['content_files'], content, 64 * 1024**2, True)
        entry = str(relative_path(config['entry']))
        if entry not in config['content_files'] or Path(entry).suffix.lower() not in ('.html', '.htm'):
            raise WorkerError('invalid_snapshot_entry')
        return binary, content, sid_text

    def launch(self, binary, sid_text):
        api = self.api
        descriptor = P()
        api.check(api.ConvertStringSecurityDescriptorToSecurityDescriptorW(
            'D:(A;;GA;;;OW)(A;;GA;;;SY)(A;;GA;;;' + sid_text + ')S:(ML;;NW;;;LW)',
            1, ct.byref(descriptor), None), 'pipe_security_failed')
        try:
            security = SecurityAttributes(ct.sizeof(SecurityAttributes), descriptor, True)
            def pipe_pair():
                reader, writer = wt.HANDLE(), wt.HANDLE()
                api.check(api.CreatePipe(ct.byref(reader), ct.byref(writer), ct.byref(security), 65536), 'pipe_create_failed')
                try:
                    read_fd = api.msvcrt.open_osfhandle(reader.value, os.O_BINARY | os.O_RDONLY)
                except BaseException:
                    api.CloseHandle(reader); api.CloseHandle(writer)
                    raise
                self.fds.append(read_fd)
                try:
                    write_fd = api.msvcrt.open_osfhandle(writer.value, os.O_BINARY | os.O_WRONLY)
                except BaseException:
                    api.CloseHandle(writer)
                    raise
                self.fds.append(write_fd)
                return read_fd, write_fd
            child_read, self.write_fd = pipe_pair()
            self.read_fd, child_write = pipe_pair()
        finally:
            api.LocalFree(descriptor)
        self.child_fds = [child_read, child_write]
        for fd in self.fds:
            api.check(api.SetHandleInformation(api.msvcrt.get_osfhandle(fd), 1, int(fd in self.child_fds)), 'pipe_inheritance_failed')
        stderr = os.open(self.folder / 'browser-errors.log', os.O_BINARY | os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        self.fds.append(stderr)
        self.child_fds.append(stderr)
        os.set_handle_inheritable(api.msvcrt.get_osfhandle(stderr), True)
        handles = (wt.HANDLE * 3)(*(api.msvcrt.get_osfhandle(fd) for fd in self.child_fds))
        ct.set_last_error(0)
        job = api.CreateJobObjectW(None, self.receipt['job'])
        api.check(job, 'job_create_failed')
        if ct.get_last_error() == 183:
            api.CloseHandle(job)
            raise WorkerError('job_identity_already_exists')
        self.job = job
        limits = Limits()
        limits.basic.flags, limits.basic.processLimit = 0x2000 | 0x200 | 0x8 | 0x20, 16
        limits.basic.priority = 0x4000
        limits.jobMemory = 2 * 1024**3
        api.check(api.SetInformationJobObject(self.job, 9, ct.byref(limits), ct.sizeof(limits)), 'job_limits_failed')
        length = SIZE()
        api.InitializeProcThreadAttributeList(None, 3, 0, ct.byref(length))
        attributes = ct.create_string_buffer(length.value)
        api.check(api.InitializeProcThreadAttributeList(attributes, 3, 0, ct.byref(length)), 'attributes_initialize_failed')
        try:
            capabilities = Capabilities(self.sid, None, 0, 0)
            api.check(api.UpdateProcThreadAttribute(attributes, 0, 0x20009, ct.byref(capabilities), ct.sizeof(capabilities), None, None), 'security_attribute_failed')
            api.check(api.UpdateProcThreadAttribute(attributes, 0, 0x20002, handles, ct.sizeof(handles), None, None), 'handle_attribute_failed')
            jobs = (wt.HANDLE * 1)(self.job)
            # PROC_THREAD_ATTRIBUTE_JOB_LIST: membership is established by
            # CreateProcess itself, eliminating the later-assignment crash gap.
            api.check(api.UpdateProcThreadAttribute(attributes, 0, 0x2000d, jobs, ct.sizeof(jobs), None, None), 'atomic_job_attribute_failed')
            startup = Extended()
            startup.startup.cb, startup.startup.flags = ct.sizeof(startup), 0x101
            startup.startup.stdin, startup.startup.stdout, startup.startup.stderr = handles
            startup.attributes = ct.cast(attributes, P)
            args = [str(binary), '--headless=new', '--no-sandbox', '--no-first-run',
                    '--disable-gpu', '--disable-background-networking', '--disable-extensions',
                    '--disable-sync', '--no-default-browser-check', '--window-size=1280,900',
                    '--user-data-dir=' + str(self.folder / 'browser'), '--remote-debugging-pipe',
                    f'--remote-debugging-io-pipes={handles[0]},{handles[1]}', 'about:blank']
            command = ct.create_unicode_buffer(subprocess.list2cmdline(args))
            temp = self.folder / 'Temp'
            temp.mkdir(exist_ok=True)
            environment = {'SystemRoot': os.environ['SystemRoot'], 'WINDIR': os.environ['SystemRoot'],
                           'PATH': str(Path(os.environ['SystemRoot']) / 'System32'),
                           'LOCALAPPDATA': str(self.folder), 'APPDATA': str(self.folder),
                           'USERPROFILE': str(self.folder), 'TEMP': str(temp), 'TMP': str(temp)}
            block = ct.create_unicode_buffer('\0'.join(f'{key}={value}' for key, value in sorted(environment.items())) + '\0\0')
            api.check(api.CreateProcessW(str(binary), command, None, None, True,
                0x08000000 | 0x80000 | 4 | 0x400 | 0x4000, block, str(binary.parent),
                ct.byref(startup), ct.byref(self.process)), 'browser_create_failed')
            owned = wt.BOOL()
            api.check(api.IsProcessInJob(self.process.process, self.job, ct.byref(owned)), 'job_membership_failed')
            if not owned.value:
                raise WorkerError('browser_not_in_atomic_job')
            self.receipt['browser'] = api.identity(self.process.process)
            self.state.save(self.receipt)
            api.token(self.process.process, self.sid)
            if api.ResumeThread(self.process.thread) == 0xffffffff:
                raise WorkerError('browser_resume_failed')
            for fd in self.child_fds:
                os.close(fd)
                self.fds.remove(fd)
            self.child_fds = []
        finally:
            api.DeleteProcThreadAttributeList(attributes)

    def audit_children(self):
        # The job forbids breakaway. Verify the identity of every observed live
        # descendant as well as the suspended root; never kill by a loose PID.
        buffer = ct.create_string_buffer(8 + 128 * ct.sizeof(SIZE))
        self.api.check(self.api.QueryInformationJobObject(self.job, 3, buffer, ct.sizeof(buffer), None), 'job_inventory_failed')
        count = ct.cast(ct.addressof(buffer) + 4, ct.POINTER(wt.DWORD)).contents.value
        if count > 128:
            raise WorkerError('job_inventory_overflow')
        pids = (SIZE * count).from_address(ct.addressof(buffer) + 8)
        for pid in pids:
            handle = self.api.OpenProcess(0x1000, False, pid)
            if not handle:
                # A process may have exited since the snapshot; query again
                # before accepting that explanation.
                fresh = ct.create_string_buffer(ct.sizeof(buffer))
                self.api.check(self.api.QueryInformationJobObject(self.job, 3, fresh, ct.sizeof(fresh), None), 'job_inventory_failed')
                n = ct.cast(ct.addressof(fresh) + 4, ct.POINTER(wt.DWORD)).contents.value
                if n > 128 or pid in (SIZE * n).from_address(ct.addressof(fresh) + 8):
                    raise WorkerError('descendant_token_unavailable')
                continue
            try:
                owned = wt.BOOL()
                self.api.check(self.api.IsProcessInJob(handle, self.job, ct.byref(owned)), 'job_membership_failed')
                if owned.value:
                    self.api.token(handle, self.sid)
                    self.checked_pids.add(int(pid))
            finally:
                self.api.CloseHandle(handle)

    def available(self, fd):
        available = wt.DWORD()
        if not self.api.PeekNamedPipe(self.api.msvcrt.get_osfhandle(fd), None, 0, None, ct.byref(available), None):
            if ct.get_last_error() in (109, 232):
                return None
            raise WorkerError('pipe_inspection_failed')
        return available.value

    def close(self):
        errors = []
        api = self.api
        if self.job:
            if not api.TerminateJobObject(self.job, 1):
                errors.append('job_termination_failed')
            deadline = time.monotonic() + 3
            while time.monotonic() < deadline:
                accounting = Accounting()
                if not api.QueryInformationJobObject(self.job, 1, ct.byref(accounting), ct.sizeof(accounting), None):
                    errors.append('job_shutdown_inventory_failed')
                    break
                if accounting.active == 0:
                    break
                time.sleep(.02)
            else:
                errors.append('job_shutdown_incomplete')
            observed_limits = Limits()
            if api.QueryInformationJobObject(self.job, 9, ct.byref(observed_limits), ct.sizeof(observed_limits), None):
                self.peak_commit = int(observed_limits.peakJob)
            api.CloseHandle(self.job)
            self.job = None
        if self.process.process:
            api.TerminateProcess(self.process.process, 1)
            if api.WaitForSingleObject(self.process.process, 3000) != 0:
                errors.append('browser_shutdown_incomplete')
            api.CloseHandle(self.process.process)
            self.process.process = None
        if self.process.thread:
            api.CloseHandle(self.process.thread)
            self.process.thread = None
        for fd in self.fds:
            try:
                os.close(fd)
            except OSError:
                errors.append('pipe_close_failed')
        self.fds = []
        if self.created:
            if not errors:
                try:
                    self.state.remove_profile(self.name)
                    self.state.forget(self.name)
                    self.receipt = None
                    self.created = False
                except (WorkerError, OSError):
                    errors.append('profile_cleanup_incomplete')
            else:
                errors.append('profile_recovery_pending')
        if self.sid:
            api.FreeSid(self.sid)
            self.sid = P()
        if self.admission:
            api.ReleaseMutex(self.admission)
            api.CloseHandle(self.admission)
            self.admission = None
        return errors


def emit(value):
    raw = json.dumps(value, ensure_ascii=True, separators=(',', ':')).encode('ascii')
    if len(raw) > MAX_FRAME:
        raise WorkerError('protocol_size_limit')
    sys.stdout.buffer.write(raw + b'\n')
    sys.stdout.buffer.flush()


def validate_config(config):
    required = {'schema', 'engine_root', 'engine_files', 'content_root', 'content_files', 'entry', 'seconds'}
    if (not isinstance(config, dict) or set(config) != required or
            type(config['schema']) is not int or config['schema'] != 1):
        raise WorkerError('invalid_worker_configuration')
    if type(config['seconds']) is not int or not 5 <= config['seconds'] <= 3600:
        raise WorkerError('invalid_session_budget')
    return config


def run():
    config = validate_config(parse_json(sys.stdin.buffer.readline(MAX_CONFIG + 1), MAX_CONFIG))
    api = Windows()
    owner = Container(api)
    outcome = None
    cleanup = []
    min_available = None
    max_profile_bytes = 0
    try:
        binary, content, sid_text = owner.prepare(config)
        owner.launch(binary, sid_text)
        owner.audit_children()
        if api.WaitForSingleObject(owner.process.process, 0) == 0:
            raise WorkerError('browser_exited_before_ready')
        emit({'type': 'ready', 'schema': 1, 'appcontainer': True, 'capabilities': 0,
              'pid': owner.process.pid, 'content_root': str(content), 'entry': config['entry'],
              'entry_url': (content / config['entry']).as_uri(),
              'profile_name': owner.name,
              'job_memory_limit': 2 * 1024**3,
              'recovered_profiles': owner.recovery['recovered'],
              'recovery_pending': owner.recovery['retained']})
        host_fd = sys.stdin.fileno()
        host, browser = bytearray(), bytearray()
        deadline = time.monotonic() + config['seconds']
        audit_at = resource_at = storage_at = 0
        closing = False
        while not closing:
            if time.monotonic() >= deadline:
                raise WorkerError('session_budget_exhausted')
            if time.monotonic() >= audit_at:
                owner.audit_children()
                audit_at = time.monotonic() + .1
            if time.monotonic() >= resource_at:
                available_ram = api.available_ram()
                min_available = available_ram if min_available is None else min(min_available, available_ram)
                if available_ram < 2 * 1024**3:
                    raise WorkerError('host_memory_pressure')
                if (owner.folder / 'browser-errors.log').stat().st_size > 2 * 1024**2:
                    raise WorkerError('browser_diagnostic_budget_exceeded')
                resource_at = time.monotonic() + .5
            if time.monotonic() >= storage_at:
                max_profile_bytes = max(max_profile_bytes, profile_usage(owner.folder))
                if shutil.disk_usage(owner.folder).free < 512 * 1024**2:
                    raise WorkerError('host_disk_pressure')
                storage_at = time.monotonic() + 2
            for fd, pending, separator, is_host in [
                (host_fd, host, b'\n', True), (owner.read_fd, browser, b'\0', False)
            ]:
                available = owner.available(fd)
                if available is None:
                    closing = True
                    break
                if available:
                    pending.extend(os.read(fd, min(available, 65536)))
                if len(pending) > MAX_FRAME:
                    raise WorkerError('protocol_size_limit')
                while separator in pending:
                    end = pending.index(separator)
                    raw = bytes(pending[:end])
                    del pending[:end + 1]
                    value = parse_json(raw, MAX_FRAME)
                    if is_host:
                        if not validate_command(value):
                            closing = True
                            break
                        wire = json.dumps(value, ensure_ascii=True, separators=(',', ':')).encode('ascii') + b'\0'
                        if len(wire) > MAX_FRAME or os.write(owner.write_fd, wire) != len(wire):
                            raise WorkerError('protocol_write_failed')
                    else:
                        if not isinstance(value, dict):
                            raise WorkerError('invalid_browser_message')
                        emit(value)
            time.sleep(.005)
    except (WorkerError, OSError, KeyError, TypeError, ValueError):
        outcome = 'isolated_browser_operation_failed'
        raise
    finally:
        cleanup = owner.close()
        if cleanup:
            emit({'type': 'worker_error', 'code': 'cleanup_incomplete', 'details': cleanup})
        elif outcome is None:
            emit({'type': 'closed', 'cleanup_complete': True, 'audited_processes': len(owner.checked_pids),
                  'peak_job_commit_bytes': owner.peak_commit,
                  'maximum_sampled_profile_bytes': max_profile_bytes,
                  'minimum_sampled_available_ram_bytes': min_available})
    if cleanup:
        raise WorkerError('cleanup_incomplete')


if __name__ == '__main__':
    sys.setrecursionlimit(4096)
    try:
        if sys.argv[1:] == ['--recover']:
            api = Windows()
            admission = api.acquire_admission()
            try:
                emit({'type': 'recovery', 'schema': 1, **RecoveryState(api).recover()})
            finally:
                api.ReleaseMutex(admission)
                api.CloseHandle(admission)
        elif sys.argv[1:]:
            raise WorkerError('unsupported_worker_arguments')
        else:
            run()
    except (WorkerError, OSError, KeyError, TypeError, ValueError) as error:
        code = str(error) if isinstance(error, WorkerError) else type(error).__name__
        try:
            emit({'type': 'worker_error', 'code': code[:160]})
        except (BrokenPipeError, OSError):
            pass
        raise SystemExit(1) from None
