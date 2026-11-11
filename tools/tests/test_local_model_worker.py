"""Fictional, tiny model blobs; no downloads or real inference."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import socket
import tempfile
import time
import unittest
from unittest.mock import patch

REPO = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('sjs_local_model_worker', REPO / 'apps/web_app/src/local_model_worker.py')
WORKER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(WORKER)


class LocalModelWorkerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='model-cache-unit-', dir=REPO / 'target')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.assertEqual(self.root.parent, (REPO / 'target').resolve())
        self.store = self.root / 'source'
        self.cache = self.root / 'cache'
        self.store.mkdir()
        self.cache.mkdir()
        config = b'{"model_type":"fictional"}'
        weights = b'GGUF fictional tiny test bytes'
        descriptors = []
        for data in (config, weights):
            digest = hashlib.sha256(data).hexdigest()
            path = self.store / 'blobs' / ('sha256-' + digest)
            path.parent.mkdir(exist_ok=True)
            path.write_bytes(data)
            descriptors.append({'digest': 'sha256:' + digest, 'size': len(data)})
        self.manifest = self.store / 'manifests-v2/ollama.com/library/fictional/vision'
        self.manifest.parent.mkdir(parents=True)
        raw = json.dumps({'config': descriptors[0], 'layers': descriptors[1:]}).encode()
        self.manifest.write_bytes(raw)
        self.choice = {'name': 'fictional:vision', 'digest': hashlib.sha256(raw).hexdigest(), 'size_bytes': WORKER.GIB, 'directory': str(self.store)}
        self.request = {'action': 'prepare', 'choice': self.choice, 'cache_root': str(self.cache)}

    def test_copy_reuse_and_source_immutability(self):
        before = {str(p.relative_to(self.store)): p.read_bytes() for p in self.store.rglob('*') if p.is_file()}
        first = WORKER.prepare(self.request)
        self.assertFalse(first['reused'])
        second = WORKER.prepare(self.request)
        self.assertTrue(second['reused'])
        self.assertEqual(first['files'], second['files'])
        self.assertEqual(before, {str(p.relative_to(self.store)): p.read_bytes() for p in self.store.rglob('*') if p.is_file()})
        self.assertFalse(list(self.cache.glob('model-stage-*')))

    def test_dangling_cache_lock_does_not_create_a_file_outside_cache(self):
        target = self.root / 'unrelated'
        lock = self.cache / 'cache.lock'
        try:
            lock.symlink_to(target)
        except OSError as error:
            self.skipTest('Host does not permit test symlinks: ' + str(getattr(error, 'winerror', error.errno)))
        with self.assertRaises(ValueError):
            with WORKER.cache_lock(lock):
                self.fail('A dangling lock must not be opened')
        self.assertFalse(target.exists())

    def test_changed_manifest_or_blob_cannot_be_admitted(self):
        self.manifest.write_bytes(self.manifest.read_bytes() + b' ')
        with self.assertRaisesRegex(ValueError, 'changed after selection'):
            WORKER.prepare(self.request)
        self.assertFalse((self.cache / self.choice['digest']).exists())

    def test_same_size_blob_corruption_and_staging_cleanup(self):
        blob = next((self.store / 'blobs').iterdir())
        blob.write_bytes(b'x' * blob.stat().st_size)
        with self.assertRaisesRegex(ValueError, 'does not match'):
            WORKER.prepare(self.request)
        self.assertFalse(list(self.cache.glob('model-stage-*')))

    def test_damaged_recognized_cache_is_preserved_and_replaced(self):
        original = WORKER.prepare(self.request)
        blob = next((Path(original['models']) / 'blobs').iterdir())
        blob.write_bytes(b'x' * blob.stat().st_size)
        repaired = WORKER.prepare(self.request)
        self.assertFalse(repaired['reused'])
        self.assertTrue((self.cache / repaired['retained_cache']).is_dir())
        self.assertTrue(WORKER.prepare(self.request)['reused'])

    def test_unknown_cache_is_not_deleted_or_overwritten(self):
        folder = self.cache / self.choice['digest']
        folder.mkdir()
        marker = folder / 'cache-record.json'
        marker.write_text('{"schema":999}', encoding='utf-8')
        with self.assertRaisesRegex(ValueError, 'unrecognized'):
            WORKER.prepare(self.request)
        self.assertEqual(marker.read_text(encoding='utf-8'), '{"schema":999}')

    def test_insufficient_disk_and_expired_copy_budget(self):
        with patch.object(WORKER.shutil, 'disk_usage', return_value=type('Usage', (), {'free': 0})()):
            with self.assertRaisesRegex(ValueError, 'disk space'):
                WORKER.prepare(self.request)
        files = WORKER.inventory(self.store, self.choice)
        relative, descriptor = next(iter(files.items()))
        with self.assertRaises(TimeoutError):
            WORKER.transfer(self.store / relative, descriptor, time.monotonic() - 1)

    def test_model_name_and_inventory_cannot_traverse(self):
        for name in ['../fictional:vision', 'fictional:../vision', 'bad\\name:vision']:
            with self.assertRaises(ValueError):
                WORKER.manifest_paths(name)
        with self.assertRaises(ValueError):
            WORKER.under(self.store, '../outside')

    @unittest.skipUnless(os.name == 'nt', 'native Windows listener table')
    def test_listener_requires_the_exact_process(self):
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            listener.listen()
            request = {'port': listener.getsockname()[1], 'pid': os.getpid()}
            self.assertEqual(WORKER.listener(request)['pid'], os.getpid())
            self.assertEqual(WORKER.listener({'port':request['port']})['owners'], [os.getpid()])
            with self.assertRaisesRegex(ValueError, 'ownership'):
                WORKER.listener({**request, 'pid': os.getpid() + 1})
        self.assertEqual(WORKER.listener({'port':request['port']})['owners'], [])

    @unittest.skipUnless(os.name == 'nt', 'native Windows process wait state')
    def test_terminated_process_with_an_open_handle_is_exited(self):
        import ctypes
        from ctypes import wintypes
        import subprocess
        import sys
        kernel=ctypes.WinDLL('kernel32',use_last_error=True)
        kernel.OpenProcess.argtypes=[wintypes.DWORD,wintypes.BOOL,wintypes.DWORD]
        kernel.OpenProcess.restype=wintypes.HANDLE
        kernel.GetProcessTimes.argtypes=[wintypes.HANDLE,*([ctypes.POINTER(wintypes.FILETIME)]*4)]
        kernel.GetProcessTimes.restype=wintypes.BOOL
        kernel.CloseHandle.argtypes=[wintypes.HANDLE]
        kernel.CloseHandle.restype=wintypes.BOOL
        child=subprocess.Popen([sys.executable,'-I','-c','import time;time.sleep(30)'],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,creationflags=subprocess.CREATE_NO_WINDOW)
        handle=None
        try:
            handle=kernel.OpenProcess(0x1000|0x00100000,False,child.pid)
            self.assertTrue(handle)
            created,exited,kernel_time,user_time=(wintypes.FILETIME() for _ in range(4))
            self.assertTrue(kernel.GetProcessTimes(handle,ctypes.byref(created),ctypes.byref(exited),ctypes.byref(kernel_time),ctypes.byref(user_time)))
            token=(created.dwHighDateTime<<32)|created.dwLowDateTime
            request={'pid':child.pid,'created':token}
            self.assertFalse(WORKER.process_state(request)['exited'])
            child.kill();child.wait(timeout=5)
            # The inspection handle deliberately retains the process object.
            # Creation identity stays available, but the wait is signaled.
            state=WORKER.process_state(request)
            self.assertTrue(state['exited']);self.assertEqual(state['reason'],'signaled')
        finally:
            if child.poll() is None:child.kill()
            child.wait(timeout=5)
            if handle:self.assertTrue(kernel.CloseHandle(handle))

    @unittest.skipUnless(os.name == 'nt', 'native Windows listener table')
    def test_ipv6_listener_is_not_mistaken_for_absence(self):
        with socket.socket(socket.AF_INET6, socket.SOCK_STREAM) as server:
            server.bind(('::1', 0))
            server.listen()
            request = {'port': server.getsockname()[1]}
            self.assertEqual(WORKER.listener(request)['owners'], [os.getpid()])
            with self.assertRaisesRegex(ValueError, 'ownership'):
                WORKER.listener({**request, 'pid': os.getpid()})
        self.assertEqual(WORKER.listener(request)['owners'], [])

    @unittest.skipUnless(os.name == 'nt', 'native Windows table layout')
    def test_wildcard_native_table_row_is_not_mistaken_for_absence(self):
        import ctypes
        from ctypes import wintypes
        import struct
        from types import SimpleNamespace
        port = 42421
        # Synthetic MIB_TCPTABLE_OWNER_PID bytes; no wildcard socket is opened.
        row = struct.pack('<7I', 1, 2, 0, socket.htons(port), 0, 0, os.getpid())
        def table(buffer, size, _ordered, family, _kind, _reserved):
            data = row if family == socket.AF_INET else struct.pack('<I', 0)
            ctypes.cast(size, ctypes.POINTER(wintypes.DWORD)).contents.value = len(data)
            if buffer is None:
                return 122
            ctypes.memmove(buffer, data, len(data))
            return 0
        with patch.object(WORKER.ctypes, 'WinDLL', return_value=SimpleNamespace(GetExtendedTcpTable=table)):
            self.assertEqual(WORKER.listener({'port': port})['owners'], [os.getpid()])
            with self.assertRaisesRegex(ValueError, 'ownership'):
                WORKER.listener({'port': port, 'pid': os.getpid()})


if __name__ == '__main__':
    unittest.main()
