"""Local, external-mirror CocoIndex Code routing for development agents.

This tool is never imported by scanner runtime. Install and index explicitly;
search refuses stale source bytes. Exact source reads remain authoritative.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
import tempfile
import uuid
from contextlib import contextmanager


ROOT = Path(__file__).resolve().parent.parent
VERSION = "0.2.41"
MODEL = "Snowflake/snowflake-arctic-embed-xs"
MODEL_REVISION = "d8c86521100d3556476a063fc2342036d45c106f"
MODEL_FILES = (
    'config_sentence_transformers.json',
    'config.json',
    'model.safetensors',
    'modules.json',
    'sentence_bert_config.json',
    'special_tokens_map.json',
    'tokenizer_config.json',
    'tokenizer.json',
    'vocab.txt',
    '1_Pooling/config.json',
)
MODEL_DIGESTS = (
    '26a4caecbc808973e5b3b3a04484db1389078b0458254e4dd617ff361e2f378a',
    'd7d071046ab952af96b7abad788db7ab3fc997b465e1b9914ff39707092254ec',
    'ee789e0b1d6ecbbd5ce37b474af556cc1a1319cee4417d9e3b11f82e90300706',
    '84e40c8e006c9b1d6c122e02cba9b02458120b5fb0c87b746c41e0207cf642cf',
    '4dfc17a899f3ea25b5c8e9034a1d0d9ebd96eca0868b015a89d1d748c38e4b2f',
    '5d5b662e421ea9fac075174bb0688ee0d9431699900b90662acd44b2a350503a',
    '9ca59277519f6e3692c8685e26b94d4afca2d5438deff66483db495e48735810',
    '91f1def9b9391fdabe028cd3f3fcc4efd34e5d1f08c3bf2de513ebb5911a1854',
    '07eced375cec144d27c900241f3e339478dec958f92fddbc551f295c992038a3',
    '83eeb6d0c128b377a452f121f8c5a19bbbf9322092a87b856ef782f4ad2326b9',
)
MODEL_HASHES = dict(zip(MODEL_FILES, MODEL_DIGESTS, strict=True))
INDEX_SUFFIXES = (".py", ".js", ".mjs", ".cjs", ".ts", ".jsx", ".tsx", ".rs", ".go",
                  ".c", ".h", ".cpp", ".cs", ".java", ".rb", ".php",
                  ".sh", ".ps1", ".html", ".css", ".md", ".toml", ".yaml", ".yml", ".json")
PROJECT_SETTINGS_V1 = ("include_patterns:\n  - '*'\n"
                       "exclude_patterns:\n  - '.cocoindex_code/**'\n"
                       "  - '**/.cocoindex_code/**'\n")
PROJECT_SETTINGS = ("include_patterns:\n" +
                    "".join(f"  - '**/*{suffix}'\n" for suffix in INDEX_SUFFIXES) +
    "exclude_patterns:\n  - '.cocoindex_code/**'\n  - '**/.cocoindex_code/**'\n")


def settings_for(model: Path) -> str:
    return ("embedding:\n  provider: sentence-transformers\n"
            f"  model: {json.dumps(str(model))}\n  device: cpu\n"
            "daemon:\n  idle_timeout_minutes: 1\n  keep_alive_with_mcp: false\n")


def config_digest(model: Path) -> str:
    return hashlib.sha256((settings_for(model) + PROJECT_SETTINGS).encode("utf-8")).hexdigest()


def home() -> Path:
    override = os.environ.get("WAYPOINT_COCO_HOME")
    if override:
        path = Path(override).expanduser()
        if not path.is_absolute():
            raise ValueError("WAYPOINT_COCO_HOME must be an absolute path")
        selected = path.resolve()
    else:
        base = (os.environ.get("LOCALAPPDATA") or os.environ.get("XDG_DATA_HOME")
                or str(Path.home() / ".local" / "share"))
        selected = (Path(base) / "Waypoint" / "agent-code").resolve()
    if selected.is_relative_to(ROOT.resolve()):
        raise ValueError("agent state must remain outside the repository")
    return selected


def paths(base: Path, repo: Path = ROOT) -> tuple[Path, Path, Path, Path]:
    digest = hashlib.sha256(str(repo.resolve()).encode("utf-8")).hexdigest()[:16]
    return (base / "venv", base / "mirrors" / digest,
            base / "config" / digest, base / "runtime" / digest)


def ccc_path(venv: Path) -> Path:
    override = os.environ.get("WAYPOINT_COCO_CCC")
    if override:
        executable = Path(override).expanduser()
        if not executable.is_absolute() or not executable.is_file():
            raise ValueError("WAYPOINT_COCO_CCC must name an existing absolute executable")
        return executable.resolve()
    return venv / ("Scripts/ccc.exe" if os.name == "nt" else "bin/ccc")


def _run(args: list[str], *, cwd: Path | None = None, timeout: int,
         env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, cwd=cwd, env=env, timeout=timeout, check=True,
                          text=True, encoding="utf-8", stdout=subprocess.PIPE,
                          stderr=subprocess.STDOUT, errors="replace")


def verify_version(command: Path) -> None:
    if not command.is_file():
        raise RuntimeError("run install before indexing or searching")
    version = _run([str(command), "version"], timeout=30).stdout.strip()
    if version != VERSION:
        raise RuntimeError(f"CocoIndex version {version}; expected {VERSION}")


def admit_index_memory() -> None:
    if os.name != 'nt':
        return
    spec = importlib.util.spec_from_file_location('waypoint_owned_process', ROOT / 'tools/owned_process.py')
    if spec is None or spec.loader is None:
        raise RuntimeError('agent memory admission is unavailable')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    module.admit_child_memory_bytes()


def model_path(base: Path) -> Path:
    override = os.environ.get("WAYPOINT_COCO_MODEL_DIR")
    model = Path(override).expanduser() if override else base / "models" / MODEL_REVISION
    if not model.is_absolute():
        raise ValueError("model directory must be absolute")
    model = model.resolve()
    if model.is_relative_to(ROOT.resolve()):
        raise ValueError("model directory must remain outside the repository")
    verify_model(model)
    return model


def verify_model(model: Path) -> None:
    for name, expected in MODEL_HASHES.items():
        path = model / name
        if not path.is_file():
            raise RuntimeError(f"embedding model file unavailable: {name}; run install")
        with path.open('rb') as source:
            digest = hashlib.file_digest(source, 'sha256').hexdigest()
        if digest != expected:
            raise RuntimeError(f"embedding model hash mismatch: {name}")


def install_model(base: Path, command: Path) -> Path:
    if os.environ.get("WAYPOINT_COCO_MODEL_DIR"):
        return model_path(base)
    target = base / 'models' / MODEL_REVISION
    if target.exists():
        return model_path(base)
    target.parent.mkdir(parents=True, exist_ok=True)
    interpreter = command.parent / ('python.exe' if os.name == 'nt' else 'python')
    if not interpreter.is_file():
        raise RuntimeError('CocoIndex interpreter unavailable for model installation')
    with tempfile.TemporaryDirectory(prefix='model-staging-', dir=target.parent) as temporary:
        stage = Path(temporary) / 'model'
        script = ('import sys; from huggingface_hub import snapshot_download; '
                  'snapshot_download(repo_id=sys.argv[1], revision=sys.argv[2], '
                  'local_dir=sys.argv[3], allow_patterns=sys.argv[4:])')
        env = dict(os.environ)
        for name in ('HF_TOKEN', 'HUGGING_FACE_HUB_TOKEN', 'HF_ENDPOINT',
                     'HF_HUB_OFFLINE', 'TRANSFORMERS_OFFLINE'):
            env.pop(name, None)
        env.update(HF_HUB_DISABLE_TELEMETRY='1', HF_HUB_DISABLE_SYMLINKS_WARNING='1',
                   HF_HOME=str(base / 'hf-download'), OMP_NUM_THREADS='2', MKL_NUM_THREADS='2')
        _run([str(interpreter), '-c', script, MODEL, MODEL_REVISION, str(stage), *MODEL_HASHES],
             timeout=900, env=env)
        verify_model(stage)
        stage.rename(target)
    return model_path(base)


def install(base: Path) -> None:
    venv = paths(base)[0]
    command = ccc_path(venv)
    if command.is_file():
        verify_version(command)
        install_model(base, command)
        print(f"CocoIndex Code {VERSION} already installed")
        return
    venv.parent.mkdir(parents=True, exist_ok=True)
    _run([sys.executable, "-m", "venv", str(venv)], timeout=300)
    python = venv / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
    requirement = f"cocoindex-code[full]=={VERSION}"
    uv = shutil.which("uv")
    if uv:
        _run([uv, "pip", "install", "--python", str(python), requirement], timeout=3600)
    else:
        _run([str(python), "-m", "pip", "install", "--disable-pip-version-check",
              "--no-input", requirement], timeout=3600)
    verify_version(command)
    install_model(base, command)
    print(f"Installed CocoIndex Code {VERSION} in {venv}")


def tracked_files(repo: Path) -> list[tuple[str, Path]]:
    raw = subprocess.run(["git", "ls-files", "-z", "--cached"], cwd=repo,
                         timeout=30, check=True, stdout=subprocess.PIPE).stdout
    result = []
    for name in (part.decode("utf-8", "surrogateescape") for part in raw.split(b"\0") if part):
        rel = PurePosixPath(name)
        if (rel.is_absolute() or not rel.parts or
                any(part in (".", "..") or ":" in part for part in rel.parts) or
                "\\" in name):
            raise ValueError(f"unsafe tracked path: {name!r}")
        if (rel.parts[0].lower() == "secrets" or rel.name.lower().startswith('.env') or
                rel.name.lower().endswith((".env", ".pem", ".key", ".p12", ".pfx"))):
            raise ValueError(f"refusing to mirror secret-like path: {name}")
        if rel.suffix.lower() not in INDEX_SUFFIXES:
            continue
        source = repo.joinpath(*rel.parts)
        if source.is_symlink() or not source.is_file():
            raise ValueError(f"tracked source is not a regular file: {name}")
        if not source.resolve().is_relative_to(repo.resolve()):
            raise ValueError(f"tracked source escapes checkout: {name}")
        if source.stat().st_size > 32 * 1024 * 1024:
            raise ValueError(f"tracked file exceeds 32 MiB index bound: {name}")
        result.append((name, source))
    return result


def source_digest(entries: list[tuple[str, Path]]) -> str:
    digest = hashlib.sha256()
    for name, source in entries:
        digest.update(name.encode("utf-8", "surrogateescape") + b"\0")
        with source.open("rb") as stream:
            for block in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(block)
        digest.update(b"\0")
    return digest.hexdigest()


def prepare_mirror(mirror: Path, entries: list[tuple[str, Path]]) -> None:
    if mirror.is_symlink():
        raise ValueError("mirror root must not be a link")
    mirror.mkdir(parents=True, exist_ok=True)
    resolved = mirror.resolve()
    wanted = set()
    for name, source in entries:
        target = mirror.joinpath(*PurePosixPath(name).parts)
        if not target.parent.resolve().is_relative_to(resolved):
            raise ValueError(f"mirror path escapes target: {name}")
        target.parent.mkdir(parents=True, exist_ok=True)
        if target.is_symlink():
            raise ValueError(f"mirror path is a link: {name}")
        wanted.add(target.resolve())
        if not target.is_file() or target.read_bytes() != source.read_bytes():
            shutil.copyfile(source, target)
    for stale in mirror.rglob("*"):
        if not stale.is_file() or ".cocoindex_code" in stale.parts:
            continue
        if stale.resolve() not in wanted:
            if not stale.resolve().is_relative_to(resolved):
                raise ValueError("stale mirror path escapes target")
            stale.unlink()


def _environment(base: Path, config: Path, runtime: Path) -> dict[str, str]:
    result = dict(os.environ)
    result.update({"COCOINDEX_CODE_DIR": str(config),
                   "COCOINDEX_CODE_RUNTIME_DIR": str(runtime),
                   "COCOINDEX_DISABLE_USAGE_TRACKING": "1",
                   "PYTHONUTF8": "1", "OMP_NUM_THREADS": "2", "MKL_NUM_THREADS": "2",
                   "HF_HUB_OFFLINE": "1", "TRANSFORMERS_OFFLINE": "1", "HF_HUB_DISABLE_TELEMETRY": "1",
                   "HF_HOME": str(base / "hf-cache")})
    return result


def index(base: Path, repo: Path = ROOT) -> None:
    venv, mirror, config, runtime = paths(base, repo)
    command = ccc_path(venv)
    verify_version(command)
    model = model_path(base)
    settings_text = settings_for(model)
    active_config_digest = config_digest(model)
    entries = tracked_files(repo)
    digest = source_digest(entries)
    marker = base / "active" / (mirror.name + ".json")
    marker.unlink(missing_ok=True)
    prepare_mirror(mirror, entries)
    settings = config / "global_settings.yml"
    settings.parent.mkdir(parents=True, exist_ok=True)
    if settings.exists() and settings.read_text(encoding="utf-8") != settings_text:
        raise RuntimeError("CocoIndex settings changed; inspect before indexing")
    settings.write_text(settings_text, encoding="utf-8")
    project = mirror / ".cocoindex_code" / "settings.yml"
    project.parent.mkdir(parents=True, exist_ok=True)
    previous = project.read_text(encoding="utf-8") if project.exists() else None
    if previous not in (None, PROJECT_SETTINGS, PROJECT_SETTINGS_V1):
        raise RuntimeError("CocoIndex project settings changed; inspect before indexing")
    project.write_text(PROJECT_SETTINGS, encoding="utf-8")
    runtime.mkdir(parents=True, exist_ok=True)
    if previous == PROJECT_SETTINGS_V1:
        # This private runtime was created by our v1 index. Reload the
        # narrower settings before reindexing; other projects have own dirs.
        _run([str(command), "daemon", "stop"], cwd=mirror, timeout=30,
             env=_environment(base, config, runtime))
    output = _run([str(command), "index"], cwd=mirror, timeout=3600,
                  env=_environment(base, config, runtime)).stdout
    match = re.search(r"Indexing: .*?error: (\d+)", output)
    if match is None or int(match.group(1)) != 0 or source_digest(tracked_files(repo)) != digest:
        raise RuntimeError("index errors or source changed during indexing; no active marker written")
    marker.parent.mkdir(parents=True, exist_ok=True)
    marker.write_text(json.dumps({"schema": 1, "source_digest": digest,
                                  "config_digest": active_config_digest,
                                  "cocoindex_code": VERSION, "files": len(entries)}) + "\n",
                      encoding="utf-8")
    print(f"Indexed {len(entries)} tracked files; source SHA-256 {digest[:16]}")
    print(output[-2000:])


def search(base: Path, query: str, limit: int, repo: Path = ROOT,
           path: str | None = None) -> None:
    if path and any(mark in path for mark in "*?["):
        raise ValueError("wildcard path filters are unreliable in CocoIndex Code 0.2.41; "
                         "search broadly, then validate files with rg")
    venv, mirror, config, runtime = paths(base, repo)
    marker = base / "active" / (mirror.name + ".json")
    if not marker.is_file():
        raise RuntimeError("no active index; run index explicitly")
    model = model_path(base)
    active_config_digest = config_digest(model)
    recorded = json.loads(marker.read_text(encoding="utf-8"))
    current = source_digest(tracked_files(repo))
    if (recorded.get("source_digest") != current or
            recorded.get("config_digest") != active_config_digest or
            recorded.get("cocoindex_code") != VERSION):
        raise RuntimeError("index is stale; run index before semantic search")
    command = ccc_path(venv)
    verify_version(command)
    fetch = max(20, limit * 5)
    args = [str(command), "search", "--limit", str(fetch), "--json"]
    if path:
        args += ["--path", path]
    output = _run(args + [query],
                  cwd=mirror, timeout=180,
                  env=_environment(base, config, runtime)).stdout
    payload = json.loads(output)
    if not payload.get("success"):
        raise RuntimeError(f"CocoIndex search failed: {payload.get('message')}")
    if source_digest(tracked_files(repo)) != current:
        raise RuntimeError("source changed during search; results discarded")
    receipts = base / "receipts" / mirror.name
    receipts.mkdir(parents=True, exist_ok=True)
    receipt = {"schema": 1, "repo_root": str(repo.resolve()), "cocoindex_code": VERSION,
               "model_revision": MODEL_REVISION, "source_digest": current, "config_digest": active_config_digest,
               "query_sha256": hashlib.sha256(query.encode('utf-8')).hexdigest(),
               "path": path, "limit": limit, "result": payload}
    with (receipts / f"search-{uuid.uuid4().hex}.json").open('x', encoding='utf-8') as file:
        json.dump(receipt, file, ensure_ascii=False)
    hits = payload.get("results", [])
    distinct = []
    seen = set()
    for hit in hits:
        if hit["file_path"] not in seen:
            distinct.append(hit)
            seen.add(hit["file_path"])
        if len(distinct) == limit:
            break
    hits = distinct
    for hit in hits:
        excerpt = " ".join(hit["content"].split())[:160]
        print(f"{hit['file_path']}:{hit['start_line']}-{hit['end_line']} "
              f"score={hit['score']:.3f} {excerpt}")
    print(f"results={len(hits)}; confirm exact paths with rg")


@contextmanager
def exclusive_lock(base: Path, name: str):
    """Reject concurrent operations on the same private index or installer."""
    folder = base / 'locks'
    folder.mkdir(parents=True, exist_ok=True)
    with (folder / f'{name}.lock').open('a+b') as lock:
        lock.seek(0, os.SEEK_END)
        if lock.tell() == 0:
            lock.write(b'0')
            lock.flush()
        lock.seek(0)
        try:
            if os.name == 'nt':
                import msvcrt
                msvcrt.locking(lock.fileno(), msvcrt.LK_NBLCK, 1)
            else:
                import fcntl
                fcntl.flock(lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError as error:
            raise RuntimeError('another operation owns this agent index') from error
        try:
            yield
        finally:
            lock.seek(0)
            if os.name == 'nt':
                msvcrt.locking(lock.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                fcntl.flock(lock.fileno(), fcntl.LOCK_UN)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    sub.add_parser("install")
    sub.add_parser("index")
    find = sub.add_parser("search")
    find.add_argument("query")
    find.add_argument("--limit", type=int, default=5)
    find.add_argument("--path", help="repository-relative glob after initial routing")
    args = parser.parse_args(argv)
    base = home()
    lock_name = 'install' if args.action == 'install' else paths(base)[1].name
    with exclusive_lock(base, lock_name):
        if args.action == "install":
            admit_index_memory()
            install(base)
        elif args.action == "index":
            admit_index_memory()
            index(base)
        else:
            if not 1 <= args.limit <= 10 or not args.query.strip():
                parser.error("search requires a query and limit between 1 and 10")
            admit_index_memory()
            search(base, args.query, args.limit, path=args.path)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError,
            subprocess.TimeoutExpired) as exc:
        if isinstance(exc, subprocess.CalledProcessError) and exc.stdout:
            print(exc.stdout[-3000:], file=sys.stderr)
        print(f"error: CocoIndex agent search: {exc}", file=sys.stderr)
        raise SystemExit(3) from None
