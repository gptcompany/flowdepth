"""Render checks for the dash-core LaunchAgent; stdlib only, not run by the Rust CI.

Run: uv run --no-project --python 3.12 python -B -m unittest discover -s dash-core/deploy -v
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


PROM = "http://10.0.0.7:30090"


def run(
    template: Path, user: str | None = "tester", prom: str | None = PROM
) -> subprocess.CompletedProcess[bytes]:
    env = {k: v for k, v in os.environ.items() if k != "USER"}
    if user is not None:
        env["USER"] = user
    url = [] if prom is None else ["--prom-url", prom]
    return subprocess.run(
        [sys.executable, str(SCRIPT), *url, str(template)],
        env=env,
        capture_output=True,
        check=False,
    )


def with_bind(bind: str) -> bytes:
    plist = plistlib.loads(TEMPLATE.read_bytes())
    plist["EnvironmentVariables"]["DASH_BIND"] = bind
    return plistlib.dumps(plist)


def write_template(tmp: str, mutate) -> Path:
    plist = plistlib.loads(TEMPLATE.read_bytes())
    mutate(plist)
    path = Path(tmp) / "variant.plist.in"
    path.write_bytes(plistlib.dumps(plist))
    return path


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
        self.assertNotIn(b"@PROM_URL@", out.stdout)

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

    def test_missing_or_invalid_prom_url_is_refused(self) -> None:
        out = run(TEMPLATE, prom=None)
        self.assertNotEqual(out.returncode, 0)
        self.assertEqual(out.stdout, b"")
        for url in [
            "",
            "10.0.0.7:30090",
            "ftp://10.0.0.7:30090",
            "http://10.0.0.7",
            "http://:30090",
            "http://u:p@10.0.0.7:30090",
            "http://10.0.0.7:notaport",
            "http://10.0.0.7:30090/prometheus",
            "http://10.0.0.7:30090/?x=1",
        ]:
            out = run(TEMPLATE, prom=url)
            self.assertNotEqual(out.returncode, 0, url)
            self.assertEqual(out.stdout, b"", url)
            self.assertIn(b"refused", out.stderr, url)

    def test_server_root_url_with_or_without_slash_is_accepted(self) -> None:
        for url in ["http://10.0.0.7:30090", "https://10.0.0.7:30090/"]:
            out = run(TEMPLATE, prom=url)
            self.assertEqual(out.returncode, 0, out.stderr)
            self.assertEqual(
                plistlib.loads(out.stdout)["EnvironmentVariables"]["DASH_PROM_URL"], url
            )

    def test_refused_url_never_echoes_its_credentials(self) -> None:
        # rev-fd6-r1 R1: no error branch may print the supplied value.
        secret = "SENTINEL-PW-4c1d"
        for tail in [
            "10.0.0.7:30090/prometheus",
            "10.0.0.7:30090/?x=1",
            "10.0.0.7:nope",
            "10.0.0.7",
        ]:
            out = run(TEMPLATE, prom=f"http://user:{secret}@{tail}")
            self.assertEqual(out.returncode, 1, tail)
            self.assertNotIn(secret.encode(), out.stdout, tail)
            self.assertNotIn(secret.encode(), out.stderr, tail)
            self.assertIn(b"refused", out.stderr, tail)

    def test_bind_override_outside_the_environment_is_refused(self) -> None:
        # rev-fd6-r1 R3: the review's counter-template, then DASH_BIND hidden elsewhere.
        binary = "/Users/@USER@/.local/share/dash-core/current/dash-core"

        def env_wrapper(p: dict) -> None:
            p["ProgramArguments"] = ["/usr/bin/env", "DASH_BIND=0.0.0.0:8790", binary]

        def extra_argument(p: dict) -> None:
            p["ProgramArguments"] = [binary, "--verbose"]

        def bind_elsewhere(p: dict) -> None:
            p["Sockets"] = {"DASH_BIND": "0.0.0.0:8790"}

        with tempfile.TemporaryDirectory(dir=HERE) as tmp:
            for mutate in [env_wrapper, extra_argument, bind_elsewhere]:
                out = run(write_template(tmp, mutate))
                self.assertEqual(out.returncode, 1, mutate.__name__)
                self.assertEqual(out.stdout, b"", mutate.__name__)
                self.assertIn(b"refused", out.stderr, mutate.__name__)

    def test_missing_user_is_refused(self) -> None:
        out = run(TEMPLATE, user=None)
        self.assertEqual(out.returncode, 1)
        self.assertEqual(out.stdout, b"")


if __name__ == "__main__":
    unittest.main()
