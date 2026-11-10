"""Native process-tree shutdown, including a controller killed before finally runs.

Only fictional sleeping Python workers are launched. A separate outer job owns
the whole experiment, including failure paths; it stays open during assertions
so it cannot make a broken inner kill-on-close implementation appear to pass.
"""
from __future__ import annotations

import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

TOOLS = Path(__file__).resolve().parents[1]
with patch.object(sys, 'path', [str(TOOLS), *sys.path]):
    from owned_process import ChildContainment, CREATE_SUSPENDED, admit_child_memory_bytes, resume_owned_child


def spawn(mode, folder):
    return subprocess.Popen(
        [sys.executable, '-B', str(Path(__file__).resolve()), mode, str(folder)],
        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        creationflags=subprocess.CREATE_NO_WINDOW | CREATE_SUSPENDED,
    )


def fixture(mode, folder):
    if mode == '--leaf':
        time.sleep(30)
        return
    child = spawn('--leaf' if mode == '--branch' else '--branch', folder)
    job = None
    try:
        if mode == '--controller':
            job = ChildContainment(child, 128 * 1024**2)
        resume_owned_child(child)
        if mode == '--branch':
            pending = folder / 'ready.tmp'
            pending.write_text(json.dumps([os.getpid(), child.pid]), encoding='utf-8')
            pending.replace(folder / 'ready.json')
            time.sleep(30)
        else:
            deadline = time.monotonic() + 20
            while not (folder / 'stop').exists():
                if time.monotonic() >= deadline:
                    raise TimeoutError('Lifecycle fixture controller expired')
                time.sleep(.01)
    finally:
        if job is not None:
            job.close()
        elif child.poll() is None:
            child.kill()
        child.wait(timeout=5)


@unittest.skipUnless(os.name == 'nt', 'Windows job objects required')
class NativeOwnedLifecycle(unittest.TestCase):
    def exercise(self, abrupt):
        kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        kernel.OpenProcess.restype = wintypes.HANDLE
        kernel.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
        kernel.WaitForSingleObject.restype = wintypes.DWORD
        kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        kernel.CloseHandle.restype = wintypes.BOOL
        handles = []
        outer = None
        with tempfile.TemporaryDirectory(prefix='sjs-owned-lifecycle-') as temp:
            folder = Path(temp)
            budget = admit_child_memory_bytes(256 * 1024**2)
            controller = spawn('--controller', folder)
            try:
                outer = ChildContainment(controller, budget)
                resume_owned_child(controller)
                deadline = time.monotonic() + 10
                while not (folder / 'ready.json').exists():
                    self.assertIsNone(controller.poll(), 'Controller exited before readiness')
                    self.assertLess(time.monotonic(), deadline, 'Owned workers never became ready')
                    time.sleep(.01)
                # Retain handles to these exact instances: PID reuse cannot
                # turn another process's state into apparent cleanup evidence.
                pids = json.loads((folder / 'ready.json').read_text(encoding='utf-8'))
                self.assertEqual(len(set(pids)), 2)
                for pid in pids:
                    handle = kernel.OpenProcess(0x00100000, False, pid)  # SYNCHRONIZE only
                    self.assertTrue(handle, 'Cannot observe owned descendant exit')
                    handles.append(handle)
                    self.assertEqual(kernel.WaitForSingleObject(handle, 0), 258, 'Descendant not live')
                if abrupt:
                    controller.kill()  # TerminateProcess: no Python finally/atexit
                else:
                    (folder / 'stop').write_bytes(b'')
                code = controller.wait(timeout=6)
                if abrupt:
                    self.assertNotEqual(code, 0)
                else:
                    self.assertEqual(code, 0)
                for handle in handles:
                    self.assertEqual(kernel.WaitForSingleObject(handle, 5000), 0,
                                     'Owned descendant survived controller shutdown')
            finally:
                try:
                    if outer is not None:
                        outer.close()
                    elif controller.poll() is None:
                        controller.kill()
                finally:
                    try:
                        controller.wait(timeout=5)
                    finally:
                        closed = [bool(kernel.CloseHandle(handle)) for handle in handles]
                        self.assertTrue(all(closed), 'Observation handle did not close')

    def test_normal_close_drains_children_and_grandchildren(self):
        self.exercise(abrupt=False)

    def test_forced_controller_exit_drains_children_and_grandchildren(self):
        self.exercise(abrupt=True)


if __name__ == '__main__':
    if len(sys.argv) == 3 and sys.argv[1] in {'--controller', '--branch', '--leaf'}:
        fixture(sys.argv[1], Path(sys.argv[2]))
    else:
        unittest.main()
