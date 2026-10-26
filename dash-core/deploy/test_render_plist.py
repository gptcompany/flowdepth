"""Render checks for the dash-core LaunchAgent; stdlib only, not run by the Rust CI.

Run: uv run --no-project python -B -m unittest discover -s dash-core/deploy -v
"""

import os
import plistlib
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPT = HERE / "render-plist.py"
TEMPLATE = HERE / "com.gobabygo.dash-core.plist.in"


def run(template: Path, user: str | None = "tester") -> subprocess.CompletedProcess[bytes]:
    env = {k: v for k, v in os.environ.items() if k != "USER"}
    if user is not None:
        env["USER"] = user
    return subprocess.run(
        [sys.executable, str(SCRIPT), str(template)], env=env, capture_output=True, check=False
    )


def with_bind(bind: str) -> bytes:
    plist = plistlib.loads(TEMPLATE.read_bytes())
    plist["EnvironmentVariables"]["DASH_BIND"] = bind
    return plistlib.dumps(plist)


class RenderTest(unittest.TestCase):
    def test_template_renders_a_loopback_agent_for_the_user(self) -> None:
        out = run(TEMPLATE)
        self.assertEqual(out.returncode, 0, out.stderr)
        plist = plistlib.loads(out.stdout)
        self.assertEqual(plist["Label"], "com.gobabygo.dash-core")
        self.assertEqual(
            plist["ProgramArguments"], ["/Users/tester/.local/share/dash-core/current/dash-core"]
        )
        self.assertEqual(
            plist["EnvironmentVariables"],
            {"DASH_BIND": "127.0.0.1:8790", "DASH_PROM_URL": "http://10.0.0.7:30090"},
        )
        self.assertTrue(plist["RunAtLoad"])
        self.assertTrue(plist["KeepAlive"])
        log = "/Users/tester/.local/state/dash-core/dash-core.log"
        self.assertEqual((plist["StandardOutPath"], plist["StandardErrorPath"]), (log, log))
        self.assertNotIn(b"@USER@", out.stdout)

    def test_non_loopback_or_portless_bind_is_refused(self) -> None:
        # Temporary templates live under the worktree, removed on exit.
        with tempfile.TemporaryDirectory(dir=HERE) as tmp:
            for bind in [
                "0.0.0.0:8790",
                "10.0.0.5:8790",
                "[::]:8790",
                "localhost:8790",
                "127.0.0.1",
            ]:
                bad = Path(tmp) / "bad.plist.in"
                bad.write_bytes(with_bind(bind))
                out = run(bad)
                self.assertNotEqual(out.returncode, 0, bind)
                self.assertEqual(out.stdout, b"", bind)
                self.assertIn(b"refused", out.stderr, bind)
            ok = Path(tmp) / "v6.plist.in"
            ok.write_bytes(with_bind("[::1]:8790"))
            self.assertEqual(run(ok).returncode, 0)

    def test_missing_user_is_refused(self) -> None:
        out = run(TEMPLATE, user=None)
        self.assertEqual(out.returncode, 1)
        self.assertEqual(out.stdout, b"")


if __name__ == "__main__":
    unittest.main()
