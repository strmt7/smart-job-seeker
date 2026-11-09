"""Memory-contained fictional UI/model qualification; never targets employer forms."""
from pathlib import Path
import argparse
import ctypes
import hashlib
import json
import os
import shutil
import tempfile
import threading
import time
from qualify_rehearsal import run, test_denominator
from owned_process import _MemoryStatus

REPO = Path(__file__).resolve().parent.parent

class RamSamples:
    """Bounded host observations; these are not exclusive application peaks."""
    def __init__(self):
        self.stop = threading.Event()
        self.samples = []
        self.errors = []
        self.started = time.monotonic()
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.kernel.GlobalMemoryStatusEx.argtypes = [ctypes.POINTER(_MemoryStatus)]
        self.kernel.GlobalMemoryStatusEx.restype = ctypes.c_int
        self.thread = threading.Thread(target=self.observe, name="history-ui-ram", daemon=True)

    def observe(self):
        while len(self.samples) < 900:
            status = _MemoryStatus(length=ctypes.sizeof(_MemoryStatus))
            if self.kernel.GlobalMemoryStatusEx(ctypes.byref(status)):
                self.samples.append({"milliseconds":round((time.monotonic()-self.started)*1000), "available_bytes":status.available_physical,"total_bytes":status.total_physical})
            elif len(self.errors) < 3:
                self.errors.append(ctypes.get_last_error())
            if self.stop.wait(0.5):
                break

    def finish(self):
        self.stop.set()
        self.thread.join(timeout=2)
        return {"scope":"sampled whole-host physical RAM; not application/GPU peak or a reservation", "interval_ms":500,"count":len(self.samples), "minimum_available_bytes":min((row["available_bytes"] for row in self.samples),default=None),"errors":self.errors,"samples":self.samples}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--node",type=Path,required=True)
    parser.add_argument("--browser",type=Path,required=True)
    parser.add_argument("--model",default="gemma4:12b-it-q4_K_M")
    parser.add_argument("--profile",choices=["varied","baseline"],default="varied")
    parser.add_argument("--layout",choices=["repeated","flat"],default="repeated")
    args = parser.parse_args()
    if os.name != "nt":
        parser.error("Windows qualification only")
    for value in [args.node,args.browser]:
        if not value.is_absolute() or not value.is_file():
            parser.error("Executables must be explicit absolute files")
    shell = shutil.which("pwsh") or shutil.which("powershell")
    if not shell:
        parser.error("PowerShell is required for the managed build")
    folder = Path(tempfile.mkdtemp(prefix="history-ui-run-",dir=REPO/"target"))
    environment = os.environ.copy()
    environment.update(WAYPOINT_HISTORY_UI_CONTAINED="1",WAYPOINT_HISTORY_UI_NODE=str(args.node),WAYPOINT_HISTORY_UI_BROWSER=str(args.browser),WAYPOINT_BROWSER=str(args.browser),WAYPOINT_VISION_BENCHMARK_MODEL=args.model,WAYPOINT_HISTORY_PIPELINE_LAYOUT=args.layout,WAYPOINT_HISTORY_UI_PROFILE=args.profile,CARGO_BUILD_JOBS="2",PYTHONUTF8="1")
    log = folder/"native.private.log"
    receipt = {"complete":False,"scope":"fictional file form, actual rendered client, production handlers and local model; no submission","layout":args.layout,"profile":args.profile,"model":args.model,"sources":{str(path.relative_to(REPO)):hashlib.sha256(path.read_bytes()).hexdigest() for path in [Path(__file__),REPO/"tools/qualify_rehearsal.py",REPO/"tools/owned_process.py"]},"browser_executable_sha256":hashlib.sha256(args.browser.read_bytes()).hexdigest()}
    memory = RamSamples()
    memory.thread.start()
    try:
        receipt["run"] = run([shell,"-NoProfile","-File",str(REPO/"apps/web_app/scripts/build-windows.ps1"),"-Test","-TestFilter","resource_gated_rendered_history_fill_and_visual_review","-Ignored","-NoCapture"],environment,log,420)
        if receipt["run"]["exit_code"]:
            raise RuntimeError("Qualification failed; inspect "+str(log))
        receipt["tests"] = test_denominator(log.read_text(encoding="utf-8"),"rust",1)
        receipt["complete"] = True
        print("Rendered workflow qualification:",folder)
    finally:
        receipt["memory"] = memory.finish()
        (folder/"result.private.json").write_text(json.dumps(receipt,indent=2),encoding="utf-8")

if __name__ == "__main__":
    main()
