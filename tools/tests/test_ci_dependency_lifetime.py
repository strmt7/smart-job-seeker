"""Run actual CI setup against a fictional SDK, then discard Cargo output."""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

REPO=Path(__file__).resolve().parents[2]
SHELL=shutil.which('pwsh') or shutil.which('powershell')

def setup_block(workflow):
    lines=(REPO/'.github/workflows'/workflow).read_text(encoding='utf-8').splitlines()
    marker=next(i for i,line in enumerate(lines) if line.strip()=='- name: Configure OpenSSL for SQLCipher')
    start=next(i for i in range(marker+1,len(lines)) if lines[i].strip()=='run: |')+1
    result=[]
    for line in lines[start:]:
        if line.strip() and not line.startswith('          '):break
        result.append(line[10:])
    if not result:raise AssertionError('Missing actual SDK setup commands')
    return '\n'.join(result)

@unittest.skipUnless(SHELL,'PowerShell required to execute actual Windows workflow setup')
class DependencyLifetimeTests(unittest.TestCase):
    def exercise(self,workflow):
        target=REPO/'target';target.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='ci-sdk-lifetime-',dir=target) as temp:
            root=Path(temp).resolve();cargo_target=root/'target';cargo_target.mkdir()
            runner=root/'runner-temp';runner.mkdir()
            scripts=root/'apps/web_app/scripts';scripts.mkdir(parents=True)
            # Only the installer is substituted. Directory selection and persisted
            # environment variables are the exact committed workflow commands.
            (scripts/'provision-openssl.ps1').write_text("""param([string]$TargetRoot)
$ErrorActionPreference='Stop'
$sdk=Join-Path (Resolve-Path -LiteralPath $TargetRoot).Path 'fictional-sdk'
New-Item -ItemType Directory -Force -Path (Join-Path $sdk 'include/openssl'),(Join-Path $sdk 'lib'),(Join-Path $sdk 'bin') | Out-Null
[IO.File]::WriteAllText((Join-Path $sdk 'include/openssl/crypto.h'),'fictional SDK header')
Write-Output $sdk
""",encoding='utf-8')
            script=root/'setup.ps1';script.write_text(setup_block(workflow),encoding='utf-8')
            env={**os.environ,'RUNNER_TEMP':str(runner),'GITHUB_ENV':str(root/'environment.txt'),'GITHUB_PATH':str(root/'path.txt')}
            result=subprocess.run([SHELL,'-NoLogo','-NoProfile','-NonInteractive','-File',str(script)],cwd=root,env=env,capture_output=True,timeout=20,creationflags=0x08000000 if os.name=='nt' else 0)
            self.assertEqual(result.returncode,0,'Actual workflow setup failed against the isolated SDK fixture')
            values=dict(line.split('=',1) for line in (root/'environment.txt').read_text(encoding='utf-8-sig').splitlines() if '=' in line)
            header=Path(values['OPENSSL_INCLUDE_DIR'])/'openssl/crypto.h'
            before=hashlib.sha256(header.read_bytes()).digest()
            # This is the test-owned output directory, never the repository's
            # target or another workload. Model cleanup more aggressively than
            # a partial-cache restore so both restore and clean are covered.
            self.assertEqual(cargo_target.resolve().parent,root)
            self.assertTrue(cargo_target.resolve().is_relative_to(root))
            shutil.rmtree(cargo_target)
            self.assertTrue(header.is_file(),'SDK headers were inside disposable Cargo output')
            self.assertEqual(hashlib.sha256(header.read_bytes()).digest(),before)
            self.assertTrue(Path(values['OPENSSL_DIR']).resolve().is_relative_to(runner))
            self.assertEqual(Path(values['OPENSSL_LIB_DIR']),Path(values['OPENSSL_DIR'])/'lib')
            self.assertIn(str(Path(values['OPENSSL_DIR'])/'bin'),(root/'path.txt').read_text(encoding='utf-8-sig'))
    def test_build_job_sdk_survives_cargo_output_cleanup(self):self.exercise('ci.yml')
    def test_coverage_sdk_survives_cargo_output_cleanup(self):self.exercise('coverage.yml')

if __name__=='__main__':unittest.main()
