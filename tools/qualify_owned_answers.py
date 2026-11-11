"""Qualify fictional answers using a separate, job-contained local Ollama runtime.

Copies only the selected hash-verified model into the disposable run folder.
Never restarts, reconfigures, loads or unloads the user's existing model server.
"""
from __future__ import annotations

import argparse
import base64
import ctypes
from ctypes import wintypes
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import time
import uuid
from urllib.request import Request, build_opener, ProxyHandler

from qualify_history_ui import RamSamples
from qualify_rehearsal import run, ensure_host_reserve, test_denominator

REPO = Path(__file__).resolve().parents[1]
TEST = 'source_answer_qualification::private_runtime_prepares_grounded_answers_and_refuses_unsupported_experience'
VISION_TEST = 'source_vision_qualification::private_runtime_transcribes_form_values_without_expected_answer_leakage'
FRAME_VISION_TEST = 'source_vision_qualification::private_runtime_reviews_recorded_frame_captures'
REHEARSAL_VISION_SUITE = 'vision-saved-rehearsal'
REHEARSAL_VISION_TEST = 'recorded_preview::tests::owned_runtime_reviews_saved_copy_rehearsal'
FRAME_VISION_SUITES = ('vision-frames','vision-frames-system','vision-frames-confirmed')
TESTS = {'answers':TEST, 'vision':VISION_TEST, 'vision-system':VISION_TEST, **dict.fromkeys(FRAME_VISION_SUITES,FRAME_VISION_TEST)}
TESTS[REHEARSAL_VISION_SUITE] = REHEARSAL_VISION_TEST
GIB = 1024**3
PROMPT_CACHE_MIB = 512
CONTEXT_CHECKPOINTS = 1


def digest(path):
    result = hashlib.sha256()
    with path.open('rb') as source:
        while data := source.read(4 * 1024**2):
            result.update(data)
    return result.hexdigest()


def local_json(origin, path, seconds=3):
    if not re.fullmatch(r'http://127\.0\.0\.1:[1-9][0-9]{0,4}', origin):
        raise ValueError('Only a literal loopback runtime is permitted')
    # Explicitly bypass inherited proxy configuration for local observations.
    with build_opener(ProxyHandler({})).open(Request(origin + path), timeout=seconds) as response:
        data = response.read(1024**2 + 1)
        if len(data) > 1024**2:
            raise RuntimeError('Local metadata response exceeds its limit')
        return json.loads(data)


def regular(path):
    if path.is_symlink() or path.is_junction() or not path.is_file():
        raise ValueError('Only regular model files may be copied')
    return path



FRAME_VISION_CASES = {
    'recorded_controls': ('controls-screenshot.base64', 4, 'matches_transcription'),
    'recorded_followup': ('followup-screenshot.base64', 1, 'matches_transcription'),
    'parent_without_child': ('replay-screenshot.base64', 1, 'unobserved'),
}


def recorded_frame_vision(report_path, require_focus=False, require_verified_text=False):
    """Bind bounded fictional images to the native rehearsal denominator."""
    target = (REPO / 'target').resolve(strict=True)
    report_path = regular(report_path).resolve(strict=True)
    folder = report_path.parent
    if folder.parent != target or not folder.name.startswith('replay-qualification-') or report_path.name != 'result.private.json':
        raise ValueError('Use a retained fictional replay result directly under target')

    def bounded(path, limit):
        checked = regular(path).resolve(strict=True)
        if checked.parent != folder:
            raise ValueError('Recorded input escaped its qualification folder')
        with checked.open('rb') as stream:
            data = stream.read(limit + 1)
        if not data or len(data) > limit:
            raise ValueError('Recorded input exceeds its byte budget')
        return data

    report_bytes = bounded(report_path, 512 * 1024)
    report = json.loads(report_bytes)
    native_bytes = bounded(folder / 'native.private.json', 128 * 1024)
    native_sha = hashlib.sha256(native_bytes).hexdigest()
    if (report.get('complete') is not True or report.get('frame_controls') is not True
            or report.get('source_engine_unchanged') is not True
            or report.get('tests') != {'executed_tests': 1, 'ignored_tests': 0}
            or report.get('native_sha256') != native_sha):
        raise ValueError('Recorded result lacks verified native frame evidence')
    native = json.loads(native_bytes)
    cases = native.get('vision_cases')
    if (native.get('complete') is not True or native.get('profile_removed') is not True
            or not isinstance(cases, list) or len(cases) != len(FRAME_VISION_CASES)):
        raise ValueError('Missing complete recorded vision criteria')
    def image(filename, expected_digest):
        encoded = bounded(folder / filename, 1024**2)
        if hashlib.sha256(encoded).hexdigest() != expected_digest:
            raise ValueError('Recorded image digest changed')
        png = base64.b64decode(encoded, validate=True)
        if len(png) < 24 or png[:16] != b'\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR' or not all(0 < size <= 4096 for size in struct.unpack('>II', png[16:24])):
            raise ValueError('Recorded image is not a bounded PNG')
        return encoded.decode('ascii')

    output = []
    seen = set()
    keys = {'id','image_file','image_base64_sha256','expected','expected_status'}
    for case in cases:
        if not isinstance(case, dict) or set(case) not in (keys,keys | {'focused_images'}):
            raise ValueError('Unexpected recorded vision case schema')
        name = case['id']
        if not isinstance(name, str) or name not in FRAME_VISION_CASES or name in seen:
            raise ValueError('Unknown or duplicate recorded vision case')
        seen.add(name)
        filename, count, status = FRAME_VISION_CASES[name]
        expected = case['expected']
        if (case['image_file'] != filename or case['expected_status'] != status
                or not isinstance(expected, list) or len(expected) != count
                or any(not isinstance(pair, list) or len(pair) != 2
                       or not all(isinstance(value, str) and 0 < len(value.encode('utf-8')) <= 1000 for value in pair)
                       for pair in expected) or len({pair[0] for pair in expected}) != count):
            raise ValueError('Invalid recorded vision criteria')
        encoded = image(filename, case['image_base64_sha256'])
        focused = case.get('focused_images',[])
        if (not isinstance(focused,list) or (focused and (name!='recorded_controls' or len(focused)!=count))
                or (require_focus and name=='recorded_controls' and len(focused)!=count)):
            raise ValueError('Recorded controls lack separate focused captures')
        focus_images = []
        focus_keys = {'label','image_file','image_base64_sha256'}
        hashes = {case['image_base64_sha256']}
        for index, item in enumerate(focused):
            filename = f'control-focus-{index}.base64'
            if (not isinstance(item,dict) or set(item) not in (focus_keys,focus_keys | {'verified_text_present'})
                    or type(item.get('verified_text_present',False)) is not bool
                    or item['label']!=expected[index][0] or item['image_file']!=filename
                    or not isinstance(item['image_base64_sha256'],str) or item['image_base64_sha256'] in hashes):
                raise ValueError('Focused capture identity is missing, repeated or mismatched')
            hashes.add(item['image_base64_sha256'])
            focus_images.append({'label':item['label'],'image_base64_sha256':item['image_base64_sha256'],
                                 'image':image(filename,item['image_base64_sha256']),
                                 'verified_text_present':item.get('verified_text_present',False)})
        if require_verified_text and name=='recorded_controls' and sum(item['verified_text_present'] for item in focus_images)!=2:
            raise ValueError('Recorded controls lack two independently verified text captures')
        output.append({k:v for k,v in case.items() if k not in ('image_file','focused_images')}
                      | {'image':encoded,'focused_images':focus_images})
    return {'schema':1,'native_sha256':native_sha,'replay_result_sha256':hashlib.sha256(report_bytes).hexdigest(),'cases':output}

def resident_models(origin):
    models = local_json(origin, '/api/ps', 1).get('models')
    if not isinstance(models, list) or len(models) > 32:
        raise RuntimeError('Resident model state is unknown')
    return models


def model_inventory(store, model, expected):
    if not re.fullmatch(r'[a-zA-Z0-9_-]+:[a-zA-Z0-9_.-]+', model) or not re.fullmatch('[a-f0-9]{64}', expected):
        raise ValueError('Use an explicit library model:tag and SHA-256 digest')
    name, tag = model.split(':')
    # Current Ollama resolves the v2 named manifest before its retained legacy
    # downgrade anchor. Those can legitimately have different byte digests.
    v2 = Path('manifests-v2/ollama.com/library') / name / tag
    relative = v2 if (store / v2).exists() else Path('manifests/registry.ollama.ai/library') / name / tag
    manifest = regular(store / relative)
    with manifest.open('rb') as reader:
        raw = reader.read(1024**2 + 1)
    if len(raw) > 1024**2 or hashlib.sha256(raw).hexdigest() != expected:
        raise ValueError('Selected manifest does not match the expected model digest')
    value = json.loads(raw)
    descriptors = [value['config'], *value['layers']]
    if not 1 <= len(descriptors) <= 32:
        raise ValueError('Unexpected selected-model descriptor count')
    files = {relative: expected}
    total = manifest.stat().st_size
    for descriptor in descriptors:
        checksum = descriptor.get('digest', '')
        if not re.fullmatch('sha256:[a-f0-9]{64}', checksum):
            raise ValueError('Invalid model blob digest')
        relative_blob = Path('blobs') / checksum.replace(':', '-')
        blob = regular(store / relative_blob)
        if type(descriptor.get('size')) is not int or descriptor['size'] != blob.stat().st_size:
            raise ValueError('Model blob size does not match its manifest')
        if relative_blob not in files:
            total += blob.stat().st_size
            files[relative_blob] = checksum.split(':')[1]
    if total > 12 * GIB:
        raise ValueError('Selected model exceeds this qualification copy budget')
    return files, total


def copy_model(store, destination, files):
    deadline = time.monotonic() + 300
    destination.mkdir()
    for relative, expected in files.items():
        ensure_host_reserve()
        source = regular(store / relative)
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        checksum = hashlib.sha256()
        with source.open('rb') as reader, target.open('xb') as writer:
            while data := reader.read(4 * 1024**2):
                ensure_host_reserve()
                if time.monotonic() >= deadline:
                    raise TimeoutError('Private model staging exceeded five minutes')
                checksum.update(data)
                writer.write(data)
        if checksum.hexdigest() != expected:
            raise ValueError('Model bytes do not match the selected manifest')


def server_environment(folder, model_store, port):
    environment = os.environ.copy()
    private_profile = folder / 'runtime-profile'
    for name in ['profile', 'local', 'roaming', 'temp']:
        (private_profile / name).mkdir(parents=True, exist_ok=True)
    environment.update(
        USERPROFILE=str(private_profile / 'profile'), LOCALAPPDATA=str(private_profile / 'local'),
        APPDATA=str(private_profile / 'roaming'), TEMP=str(private_profile / 'temp'), TMP=str(private_profile / 'temp'),
        OLLAMA_HOST=f'127.0.0.1:{port}', OLLAMA_MODELS=str(model_store), OLLAMA_NO_CLOUD='1',
        OLLAMA_NOPRUNE='1', OLLAMA_NUM_PARALLEL='1', OLLAMA_MAX_LOADED_MODELS='1',
        OLLAMA_MAX_QUEUE='1', OLLAMA_CONTEXT_LENGTH='4096', OLLAMA_KEEP_ALIVE='5m',
        LLAMA_ARG_CACHE_RAM=str(PROMPT_CACHE_MIB),
        LLAMA_ARG_CTX_CHECKPOINTS=str(CONTEXT_CHECKPOINTS),
    )
    return environment


def cleanup_copies(folder):
    if (folder.is_symlink() or folder.is_junction() or folder.resolve().parent != (REPO / 'target').resolve()
            or not folder.name.startswith('owned-answer-qualification-')):
        raise RuntimeError('Qualification cleanup root changed')
    for name in ['models','runtime-profile']:
        path = folder / name
        if path.exists():
            if path.is_symlink() or path.is_junction() or path.resolve().parent != folder.resolve():
                raise RuntimeError('Private-model cleanup path changed')
            shutil.rmtree(path)


class TcpRow(ctypes.Structure):
    _fields_ = [(name, wintypes.DWORD) for name in ['state','local_addr','local_port','remote_addr','remote_port','pid']]


def listener_owner(port):
    if type(port) is not int or not 1 <= port <= 65535:
        raise ValueError('Invalid private port')
    api = ctypes.WinDLL('iphlpapi', use_last_error=True)
    query = api.GetExtendedTcpTable
    query.argtypes = [ctypes.c_void_p, ctypes.POINTER(wintypes.DWORD), wintypes.BOOL, wintypes.ULONG, ctypes.c_int, wintypes.ULONG]
    query.restype = wintypes.DWORD
    size = wintypes.DWORD()
    code = query(None, ctypes.byref(size), False, socket.AF_INET, 3, 0)  # OWNER_PID_LISTENER
    if code not in [0, 122]: raise RuntimeError('Native TCP table sizing failed')
    for _ in range(3):
        if not 4 <= size.value <= 4 * 1024**2: raise RuntimeError('Native TCP table exceeds its bounds')
        buffer = ctypes.create_string_buffer(size.value)
        code = query(buffer, ctypes.byref(size), False, socket.AF_INET, 3, 0)
        if code == 122: continue
        if code: raise RuntimeError('Native TCP ownership query failed')
        count = wintypes.DWORD.from_buffer(buffer).value
        if count > (len(buffer) - 4) // ctypes.sizeof(TcpRow): raise RuntimeError('Invalid native TCP row count')
        matches = []
        for index in range(count):
            row = TcpRow.from_buffer(buffer, 4 + index * ctypes.sizeof(TcpRow))
            if socket.ntohs(row.local_port & 0xffff) == port and socket.inet_ntoa(struct.pack('<I',row.local_addr)) == '127.0.0.1':
                # Ownership is checked independently of the state field. The
                # native listener table reported state zero on this host;
                # HTTP version/tags checks separately establish readiness.
                matches.append({'pid':row.pid,'tcp_state':row.state})
        if len(matches) != 1 or matches[0]['pid'] <= 0: raise RuntimeError('Private endpoint owner is ambiguous or missing')
        return matches[0]
    raise RuntimeError('Native TCP table changed beyond the retry budget')


def listener_identity(shell, port, expected_pid):
    owner = listener_owner(port)
    if owner['pid'] != expected_pid: raise RuntimeError('Private listener belongs to another process')
    environment = {**os.environ, 'SJS_OWNED_PID': str(expected_pid)}
    script = "$ErrorActionPreference='Stop'; $p=Get-CimInstance Win32_Process -Filter ('ProcessId='+$env:SJS_OWNED_PID); if($null -eq $p){throw 'Listener process unavailable'}; [pscustomobject]@{pid=$p.ProcessId;parent=$p.ParentProcessId;created=$p.CreationDate.ToUniversalTime().ToString('o');executable=$p.ExecutablePath}|ConvertTo-Json -Compress"
    result = subprocess.run([shell, '-NoProfile', '-NonInteractive', '-Command', script],
        env=environment, stdin=subprocess.DEVNULL, capture_output=True, timeout=10,
        creationflags=subprocess.CREATE_NO_WINDOW)
    if result.returncode:
        detail = result.stderr.decode('utf-8', errors='replace')[-2000:]
        raise RuntimeError('Private listener identity could not be verified: ' + detail)
    identity = json.loads(result.stdout)
    if identity['pid'] != expected_pid or not identity['created'] or not identity['executable']:
        raise RuntimeError('Private listener belongs to another process')
    identity['tcp_state'] = owner['tcp_state']
    return identity


def wait_runtime_ready(server, origin, config, errors):
    """An observation timeout is not an exited server or permission to restart it."""
    deadline = time.monotonic() + 60
    attempts = 0
    while time.monotonic() < deadline:
        ensure_host_reserve()
        if server.poll() is not None: raise RuntimeError('Owned runtime exited during readiness checks')
        attempts += 1
        try:
            version = local_json(origin, '/api/version', 2)
            if version.get('version') != config['version']:
                raise RuntimeError('Private runtime version differs from the selected executable')
            tags = local_json(origin, '/api/tags', 2).get('models')
            if not isinstance(tags, list): raise ValueError('Model inventory is not available yet')
            if len(tags)==1 and tags[0]['name']==config['model'] and tags[0]['digest']==config['digest']:
                return attempts
            if tags: raise RuntimeError('Private runtime model inventory differs from the selected copy')
            if len(errors)<8: errors.append('Model inventory not ready')
        except (OSError, ValueError) as error:
            if len(errors)<8: errors.append(type(error).__name__)
        time.sleep(.2)
    raise RuntimeError('Private runtime metadata did not become ready within sixty seconds')


def worker(config_path):
    config_path = regular(config_path).resolve(strict=True)
    if not config_path.is_relative_to((REPO / 'target').resolve()) or config_path.stat().st_size > 16384:
        raise ValueError('Worker configuration must be a bounded private target record')
    config = json.loads(config_path.read_text(encoding='utf-8'))
    if not config.get('nonce') or config['nonce'] != os.environ.get('WAYPOINT_OWNED_ANSWER_NONCE'):
        raise RuntimeError('Use the contained qualification controller')
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel.GetCurrentProcess.restype = wintypes.HANDLE
    kernel.IsProcessInJob.argtypes = [wintypes.HANDLE, wintypes.HANDLE, ctypes.POINTER(wintypes.BOOL)]
    kernel.IsProcessInJob.restype = wintypes.BOOL
    contained = wintypes.BOOL()
    if not kernel.IsProcessInJob(kernel.GetCurrentProcess(), None, ctypes.byref(contained)) or not contained.value:
        raise RuntimeError('Owned runtime requires Windows process-tree containment')
    folder = config_path.parent
    suite = config.get('suite','answers')
    test_name = TESTS[suite]
    runtime_log = folder / 'ollama.private.log'
    server = client = None
    observations = []
    receipt = {'complete': False, 'scope': 'owned private runtime only'}
    receipt['listener_errors'] = []
    with socket.socket() as reservation:
        reservation.bind(('127.0.0.1', 0))
        port = reservation.getsockname()[1]
    origin = f'http://127.0.0.1:{port}'
    try:
        with runtime_log.open('xb') as output:
            server = subprocess.Popen([config['ollama'], 'serve'], env=server_environment(folder, folder / 'models', port),
                stdin=subprocess.DEVNULL, stdout=output, stderr=subprocess.STDOUT,
                creationflags=subprocess.CREATE_NO_WINDOW)
            # Observe a listener before sending HTTP. The opened root process
            # handle and verified TCP owner bind requests to this runtime.
            deadline = time.monotonic() + 30
            while True:
                if server.poll() is not None or time.monotonic() >= deadline:
                    raise RuntimeError('Private runtime did not become ready')
                try:
                    receipt['identity'] = listener_identity(config['shell'], port, server.pid)
                    break
                except RuntimeError as error:
                    if len(receipt['listener_errors']) < 8: receipt['listener_errors'].append(str(error))
                    time.sleep(.2)
            receipt['origin'] = origin
            receipt['readiness_errors']=[]
            receipt['readiness_attempts']=wait_runtime_ready(server,origin,config,receipt['readiness_errors'])
            environment = {**os.environ, 'WAYPOINT_OWNED_OLLAMA_ORIGIN': origin,
                'WAYPOINT_OWNED_OLLAMA_MODEL': config['model'],
                'WAYPOINT_OWNED_ANSWER_REPORT': str(folder / 'answers.private.json'),
                'WAYPOINT_OWNED_VISION_REPORT': str(folder / (suite + '.private.json')),
                'WAYPOINT_OWNED_VISION_MODE': 'confirmed' if suite=='vision-frames-confirmed' else 'system' if suite in ('vision-system','vision-frames-system') else 'raw',
                'PATH':str(folder / 'test-runtime') + os.pathsep + os.environ.get('PATH','')}
            if suite in ('vision','vision-system'): environment['WAYPOINT_BROWSER']=config['browser']
            if suite in FRAME_VISION_SUITES: environment['WAYPOINT_OWNED_FRAME_VISION_SHA256']=config['frame_input_sha256']
            if suite == REHEARSAL_VISION_SUITE:
                environment['WAYPOINT_OWNED_REHEARSAL_SHA256'] = config['rehearsal_input_sha256']
            client_log = folder / (suite + '.private.log')
            with client_log.open('xb') as log:
                client = subprocess.Popen([config['test_binary'], test_name, '--ignored', '--exact', '--nocapture', '--test-threads=1'],
                    env=environment, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
                    creationflags=subprocess.CREATE_NO_WINDOW)
                deadline = time.monotonic() + 1100
                checked = 0.0
                while client.poll() is None:
                    ensure_host_reserve()
                    if server.poll() is not None:
                        raise RuntimeError('Owned model runtime exited before qualification completed')
                    if time.monotonic() > deadline or runtime_log.stat().st_size > 8 * 1024**2 or client_log.stat().st_size > 8 * 1024**2:
                        raise RuntimeError('Private qualification exceeded its time/output budget')
                    if time.monotonic() - checked >= 2:
                        if resident_models('http://127.0.0.1:11434'):
                            raise RuntimeError('Another model workload became active; stopped only owned work')
                        observations.append(local_json(origin, '/api/ps', 1))
                        checked = time.monotonic()
                    time.sleep(.2)
                receipt['test_exit_code'] = client.wait(timeout=5)
                if receipt['test_exit_code']:
                    raise RuntimeError('Local model criteria failed; inspect retained observations')
                receipt['denominator'] = test_denominator(client_log.read_text(encoding='utf-8'), 'rust', 1)
                receipt['complete'] = True
    finally:
        errors = []
        for process in [client, server]:
            if process is not None:
                try:
                    if process.poll() is None: process.kill()
                    process.wait(timeout=5)
                except (OSError, subprocess.TimeoutExpired): errors.append('Owned root exit unconfirmed')
        receipt['cleanup_errors'] = errors
        receipt['complete'] = receipt['complete'] and not errors
        receipt['resident_samples'] = observations
        (folder / 'runtime.private.json').write_text(json.dumps(receipt, indent=2), encoding='utf-8')
        if errors: raise RuntimeError('; '.join(errors))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ollama', type=Path, required=True)
    parser.add_argument('--model-store', type=Path, required=True)
    parser.add_argument('--model', default='gemma4:12b-it-q4_K_M')
    parser.add_argument('--digest', required=True)
    parser.add_argument('--suite', choices=sorted(TESTS), default='answers')
    parser.add_argument('--browser', type=Path)
    parser.add_argument('--replay-result', type=Path, help='Retained fictional --frame-controls result for vision-frames')
    parser.add_argument('--recorded-rehearsal-fixture', type=Path, help='Private saved-copy fixture configuration for vision-saved-rehearsal')
    args = parser.parse_args()
    if (args.suite in FRAME_VISION_SUITES) != (args.replay_result is not None):
        parser.error('A replay result is required only for recorded-frame vision suites')
    if (args.suite == REHEARSAL_VISION_SUITE) != (args.recorded_rehearsal_fixture is not None):
        parser.error('A saved-copy fixture is required only for vision-saved-rehearsal')
    rehearsal_input = None
    if args.recorded_rehearsal_fixture:
        fixture_path = regular(args.recorded_rehearsal_fixture).resolve(strict=True)
        if not fixture_path.is_relative_to((REPO / 'target').resolve()) or fixture_path.stat().st_size > 128 * 1024:
            parser.error('Use a bounded fictional saved-copy fixture under target')
        rehearsal_input = fixture_path.read_bytes()
    frame_input = recorded_frame_vision(args.replay_result, args.suite!='vision-frames', args.suite=='vision-frames-confirmed') if args.replay_result else None
    if os.name != 'nt' or not args.ollama.is_absolute() or not args.ollama.is_file():
        parser.error('Select an explicit existing Windows Ollama executable')
    if args.suite in ('vision','vision-system') and (not args.browser or not args.browser.is_absolute() or not args.browser.is_file()):
        parser.error('Vision qualification requires an explicit headless-capable browser executable')
    shell = shutil.which('pwsh') or shutil.which('powershell')
    if not shell: parser.error('A Windows PowerShell runtime is required')
    if resident_models('http://127.0.0.1:11434'):
        parser.error('A shared model is already active; no qualification was started')
    version = local_json('http://127.0.0.1:11434', '/api/version')['version']
    store = args.model_store.resolve(strict=True)
    files, total = model_inventory(store, args.model, args.digest)
    if shutil.disk_usage(REPO / 'target').free < total + 2 * GIB:
        parser.error('Insufficient disk space for a disposable model copy and 2 GiB reserve')
    folder = Path(tempfile.mkdtemp(prefix='owned-answer-qualification-', dir=REPO / 'target'))
    receipt = {'complete': False, 'started_at': datetime.now(timezone.utc).isoformat(), 'model':args.model,
        'model_digest':args.digest,'model_bytes':total,'version':version,'ollama_sha256':digest(args.ollama),'suite':args.suite}
    receipt['configured_runner_cache'] = {'ram_limit_mib':PROMPT_CACHE_MIB,'context_checkpoints':CONTEXT_CHECKPOINTS,
        'scope':'owned child runtime only; inspect native logs for backend acceptance'}
    if args.browser: receipt['browser_sha256']=digest(args.browser)
    paths = [Path(__file__), REPO / 'tools/owned_process.py', REPO / 'tools/qualify_rehearsal.py',
        REPO / 'apps/web_app/src/source_answer_qualification.rs', REPO / 'apps/web_app/src/answer_task.rs',
        REPO / 'apps/web_app/src/source_answer.rs', REPO / 'crates/waypoint_inference/src/ollama.rs',
        REPO / 'apps/web_app/src/main.rs', REPO / 'apps/web_app/src/fit_task.rs',
        REPO / 'apps/web_app/src/draft_task.rs', REPO / 'apps/web_app/src/gpu_resources.rs',
        REPO / 'Cargo.lock', REPO / 'Cargo.toml', REPO / 'apps/web_app/Cargo.toml',
        REPO / 'apps/web_app/scripts/build-windows.ps1', REPO / 'apps/web_app/src/visual_review.rs',
        REPO / 'apps/web_app/src/source_vision_qualification.rs', REPO / 'crates/waypoint_browser/src/lib.rs',
        REPO / 'crates/waypoint_browser/src/cdp.rs']
    if rehearsal_input is not None:
        paths.extend([REPO / 'apps/web_app/src/recorded_preview.rs',
            REPO / 'crates/waypoint_browser/src/captured_frame.rs'])
    hashes = lambda: {p.relative_to(REPO).as_posix(): digest(p) for p in paths}
    receipt['sources'] = hashes()
    memory = RamSamples(); memory.thread.start()
    try:
        environment = {**os.environ, 'RUST_TEST_THREADS':'4', 'CARGO_BUILD_JOBS':'2',
            'CARGO_TERM_COLOR':'never', 'CARGO_TARGET_DIR':str(REPO / 'target')}
        build_command = [shell, '-NoProfile', '-NonInteractive', '-File', str(REPO / 'apps/web_app/scripts/build-windows.ps1')]
        receipt['application_build'] = run(build_command, environment, folder / 'app-build.private.log', 600)
        if receipt['application_build']['exit_code']: raise RuntimeError('Managed application build failed')
        applications = re.findall(r'^Built (.+waypoint_web\.exe) with SQLCipher;', (folder / 'app-build.private.log').read_text(encoding='utf-8'), re.MULTILINE)
        if len(applications) != 1: raise RuntimeError('Managed build did not identify its executable')
        application = Path(applications[0]).resolve(strict=True)
        if not application.is_relative_to((REPO / 'target').resolve()): raise RuntimeError('Unexpected application build directory')
        build_receipt = json.loads((application.parent / 'waypoint-build.json').read_text(encoding='utf-8-sig'))
        dll_name = build_receipt['openssl_dll']
        if not re.fullmatch(r'[a-zA-Z0-9_.-]+\.dll', dll_name): raise RuntimeError('Invalid managed runtime name')
        dll = regular(application.parent / dll_name)
        if digest(dll) != build_receipt['openssl_sha256']: raise RuntimeError('Managed runtime bytes changed')
        runtime_folder = folder / 'test-runtime'; runtime_folder.mkdir()
        shutil.copyfile(dll, runtime_folder / dll_name)
        receipt['openssl_runtime_sha256'] = digest(runtime_folder / dll_name)
        if receipt['openssl_runtime_sha256'] != build_receipt['openssl_sha256']: raise RuntimeError('Private runtime copy changed')
        receipt['build'] = run([*build_command, '-Test', '-NoRun'], environment, folder / 'build.private.log', 600)
        if receipt['build']['exit_code']: raise RuntimeError('Managed qualification build failed')
        build = (folder / 'build.private.log').read_text(encoding='utf-8')
        receipt['test_compilation_only'] = True
        binaries = re.findall(r'Executable unittests .*? \(([^\r\n]+\.exe)\)', build)
        if len(binaries) != 1: raise RuntimeError('Managed build did not identify one native test binary')
        binary = (REPO / binaries[0]).resolve(strict=True)
        if not binary.is_relative_to((REPO / 'target').resolve()): raise RuntimeError('Unexpected test binary path')
        receipt['test_binary_sha256'] = digest(binary)
        copy_model(store, folder / 'models', files)
        config = {'ollama':str(args.ollama), 'shell':shell, 'model':args.model, 'digest':args.digest, 'nonce':uuid.uuid4().hex,
            'version':version, 'test_binary':str(binary),'suite':args.suite,'browser':str(args.browser) if args.browser else None}
        if frame_input is not None:
            input_path = folder / 'frame-vision-input.private.json'
            input_path.write_text(json.dumps(frame_input), encoding='utf-8')
            config['frame_input_sha256'] = digest(input_path)
            receipt['frame_input'] = {k:v for k,v in frame_input.items() if k != 'cases'} | {'cases':len(frame_input['cases']), 'sha256':config['frame_input_sha256']}
        if rehearsal_input is not None:
            input_path = folder / 'recorded-rehearsal-input.private.json'
            input_path.write_bytes(rehearsal_input)
            config['rehearsal_input_sha256'] = digest(input_path)
            receipt['rehearsal_input_sha256'] = config['rehearsal_input_sha256']
        config_path = folder / 'runtime-config.private.json'
        config_path.write_text(json.dumps(config), encoding='utf-8')
        environment['WAYPOINT_OWNED_ANSWER_NONCE'] = config['nonce']
        receipt['run'] = run([sys.executable, '-B', str(Path(__file__).resolve()), '--worker', str(config_path)],
            environment, folder / 'worker.private.log', 1200, memory_limit_bytes=10 * GIB)
        if receipt['run']['exit_code']: raise RuntimeError('Private model qualification failed; inspect ' + str(folder))
        if hashes() != receipt['sources']: raise RuntimeError('Qualification sources changed during execution')
        if not json.loads((folder / 'runtime.private.json').read_text())['complete']:
            raise RuntimeError('Private runtime did not confirm completion')
        receipt['complete'] = True
    finally:
        receipt['memory'] = memory.finish()
        copied = folder / 'models'
        cleanup_error = None
        try:
            cleanup_copies(folder)
        except (RuntimeError, OSError) as error:
            cleanup_error = str(error)
            receipt['cleanup_error'] = cleanup_error
            receipt['complete'] = False
        receipt['private_model_copy_removed'] = not copied.exists()
        receipt['private_profile_removed'] = not (folder / 'runtime-profile').exists()
        receipt['ended_at'] = datetime.now(timezone.utc).isoformat()
        (folder / 'result.private.json').write_text(json.dumps(receipt, indent=2), encoding='utf-8')
        print('Owned answer qualification:', folder)
        if cleanup_error: raise RuntimeError(cleanup_error)


if __name__ == '__main__':
    if len(sys.argv) == 3 and sys.argv[1] == '--worker': worker(Path(sys.argv[2]))
    else: main()
