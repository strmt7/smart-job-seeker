"""Exercise the real Windows browser-test observers with fictional process rows."""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import unittest

REPO = Path(__file__).resolve().parents[2]
SHELL = shutil.which('powershell') or shutil.which('pwsh')


@unittest.skipUnless(SHELL, 'PowerShell required for Windows process observers')
class ProcessAncestryTests(unittest.TestCase):
    def observe(self, rows, observer=0, identities=None):
        source = (REPO / 'crates/waypoint_browser/tests/cdp_live.rs').read_text(encoding='utf-8')
        source = source.split('fn forced_parent_termination_closes_owned_browser_tree() {', 1)[1]
        source = source.split('\n#[test]', 1)[0]
        scripts = re.findall(r'\.args\(\["-NoProfile", "-NonInteractive", "-Command", ("(?:[^"\\]|\\.)*")\]\)', source)
        self.assertEqual(len(scripts), 2, 'Expected the actual snapshot and identity-check observers')
        # Only CIM enumeration is substituted; ancestry and identity checks are
        # the exact Rust fixture commands. The observer must still see itself.
        fixture = """
function Get-CimInstance {
    param($Class)
    $rows = ConvertFrom-Json -InputObject $env:WAYPOINT_FAKE_ROWS
    foreach ($row in $rows) {
        $row.CreationDate = if ($null -eq $row.CreationDate) { $null } else { [datetime]::new([long]$row.CreationDate, [DateTimeKind]::Utc) }
        Write-Output $row
    }
    [pscustomobject]@{ProcessId=$PID;ParentProcessId=0;Name='powershell.exe';CommandLine='fixture observer';CreationDate=[datetime]::UtcNow}
}
"""
        env = {**os.environ, 'WAYPOINT_TEST_PROFILE': 'fictional-profile',
               'WAYPOINT_FAKE_ROWS': json.dumps(rows),
               'WAYPOINT_TEST_IDENTITIES': json.dumps(identities or [])}
        result = subprocess.run([SHELL, '-NoProfile', '-NonInteractive', '-Command',
                                 fixture + json.loads(scripts[observer])], env=env,
                                capture_output=True, timeout=15,
                                creationflags=0x08000000 if os.name == 'nt' else 0)
        return result

    @staticmethod
    def row(pid, parent, created, name='chrome.exe', command='fixture child'):
        return dict(ProcessId=pid, ParentProcessId=parent, CreationDate=created,
                    Name=name, CommandLine=command)

    def test_reused_parent_pid_cannot_adopt_older_process_or_its_children(self):
        rows = [self.row(100, 9, 1000, command='--user-data-dir=fictional-profile'),
                self.row(101, 100, 1001), self.row(102, 101, 1002),
                self.row(103, 100, 900, 'python.exe'),
                self.row(104, 103, 901, 'python.exe'),
                self.row(105, 101, 950, 'python.exe'),
                self.row(106, 105, 951, 'python.exe'),
                self.row(107, 101, 1003, 'python.exe')]
        result = self.observe(rows)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual({row['pid'] for row in json.loads(result.stdout)}, {100, 101, 102, 107})

    def test_missing_descendant_creation_time_fails_observation(self):
        result = self.observe([self.row(100, 9, 1000, command='fictional-profile'),
                               self.row(101, 100, None)])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'creation time unavailable', result.stderr)

    def test_missing_root_creation_time_fails_observation(self):
        result = self.observe([self.row(100, 9, None, command='fictional-profile')])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'creation time unavailable', result.stderr)

    def test_survivor_check_requires_same_pid_and_creation_time(self):
        rows = [self.row(100, 9, 2000), self.row(101, 9, 1001)]
        identities = [dict(pid=100, created='1000', name='chrome.exe'),
                      dict(pid=101, created='1001', name='chrome.exe')]
        result = self.observe(rows, observer=1, identities=identities)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([row['pid'] for row in json.loads(result.stdout)], [101])


if __name__ == '__main__':
    unittest.main()
