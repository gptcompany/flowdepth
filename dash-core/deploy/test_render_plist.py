"""Render checks for the dash-core LaunchAgent; stdlib only, not run by the Rust CI.

Run: uv run --no-project --python 3.12 python -B -m unittest discover -s dash-core/deploy -v
"""

import os
import plistlib
import subprocess
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPT = HERE / "render-plist.py"
README = HERE.parent / "README.md"
PROM = "http://10.0.0.7:30090"
HOME = "/Users/tester"
KEYS = {
    "Label",
    "ProgramArguments",
    "EnvironmentVariables",
    "RunAtLoad",
    "KeepAlive",
    "StandardOutPath",
    "StandardErrorPath",
}


def run(
    prom: str | None = PROM, home: str | None = HOME, extra: tuple[str, ...] = ()
) -> subprocess.CompletedProcess[bytes]:
    env = {k: v for k, v in os.environ.items() if k != "HOME"}
    if home is not None:
        env["HOME"] = home
    url = [] if prom is None else ["--prom-url", prom]
    return subprocess.run(
        [sys.executable, str(SCRIPT), *url, *extra], env=env, capture_output=True, check=False
    )


class RenderTest(unittest.TestCase):
    def test_agent_has_exactly_the_fixed_keys(self) -> None:
        out = run()
        self.assertEqual(out.returncode, 0, out.stderr)
        plist = plistlib.loads(out.stdout)
        self.assertEqual(set(plist), KEYS)
        self.assertNotIn("Program", plist)
        self.assertEqual(plist["Label"], "com.gobabygo.dash-core")
        self.assertEqual(
            plist["ProgramArguments"], ["/Users/tester/.local/share/dash-core/current/dash-core"]
        )
        self.assertEqual(
            plist["EnvironmentVariables"],
            {"DASH_BIND": "127.0.0.1:8790", "DASH_PROM_URL": PROM},
        )
        self.assertIs(plist["RunAtLoad"], True)
        self.assertIs(plist["KeepAlive"], True)
        log = "/Users/tester/.local/state/dash-core/dash-core.log"
        self.assertEqual((plist["StandardOutPath"], plist["StandardErrorPath"]), (log, log))

    def test_no_free_form_input_is_accepted(self) -> None:
        # rev-fd6-r2: no template argument exists, so no key can be added from outside.
        out = run(extra=("com.gobabygo.dash-core.plist.in",))
        self.assertEqual(out.returncode, 2)
        self.assertEqual(out.stdout, b"")

    def test_missing_or_invalid_prom_url_is_refused(self) -> None:
        out = run(prom=None)
        self.assertEqual(out.returncode, 2)
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
            out = run(prom=url)
            self.assertEqual(out.returncode, 1, url)
            self.assertEqual(out.stdout, b"", url)
            self.assertIn(b"refused", out.stderr, url)

    def test_refused_url_never_echoes_its_credentials(self) -> None:
        # rev-fd6-r1 R1: no error branch may print the supplied value.
        secret = "SENTINEL-PW-4c1d"
        for tail in [
            "10.0.0.7:30090/prometheus",
            "10.0.0.7:30090/?x=1",
            "10.0.0.7:nope",
            "10.0.0.7",
        ]:
            out = run(prom=f"http://user:{secret}@{tail}")
            self.assertEqual(out.returncode, 1, tail)
            self.assertNotIn(secret.encode(), out.stdout, tail)
            self.assertNotIn(secret.encode(), out.stderr, tail)

    def test_server_root_url_with_or_without_slash_is_accepted(self) -> None:
        for url in ["http://10.0.0.7:30090", "https://10.0.0.7:30090/"]:
            out = run(prom=url)
            self.assertEqual(out.returncode, 0, out.stderr)
            env = plistlib.loads(out.stdout)["EnvironmentVariables"]
            self.assertEqual(env["DASH_PROM_URL"], url)

    def test_missing_or_relative_home_is_refused(self) -> None:
        for home in [None, "", "relative/home"]:
            out = run(home=home)
            self.assertEqual(out.returncode, 1, home)
            self.assertEqual(out.stdout, b"", home)

    def test_readme_creates_launchagents_before_writing_the_plist(self) -> None:
        # rev-fd6-r1 R2: on a fresh account the redirect to $P.new needs the directory.
        text = README.read_text()
        mkdir = text.index("mkdir -p ~/Library/LaunchAgents")
        self.assertLess(mkdir, text.index('> "$P.new"'))


if __name__ == "__main__":
    unittest.main()
