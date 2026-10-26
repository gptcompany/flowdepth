#!/usr/bin/env python3
"""Write the dash-core LaunchAgent plist; no template, no free-form input.

Derived from gptcompany/gbg-agents scripts/metrics/render-plist.py (plistlib, no shell or XML
substitution), but the plist is built here as a fixed dict: a template accepted key by key is a
blocklist that any new launchd key bypasses (review rev-fd6-r2, `Program`). The only inputs are
--prom-url and HOME. The bind is fixed to loopback because dash-core has no auth in phase 1
(gbg-agents#883); the Prometheus URL has no default because the primary's node changes
(monitoring-stack runbook).
"""

import argparse
import os
import plistlib
import sys
from pathlib import Path
from urllib.parse import urlsplit

LABEL = "com.gobabygo.dash-core"
BIND = "127.0.0.1:8790"


def prom_url_error(url: str) -> str | None:
    """Fixed messages only: the supplied URL may carry a secret and is never echoed."""
    try:
        parts = urlsplit(url)
        # Credentials never belong in an agent definition; checked before .port can raise.
        if parts.username or parts.password:
            return "--prom-url must not carry credentials"
        port = parts.port
    except ValueError:
        return "--prom-url is not a valid URL"
    if parts.scheme not in ("http", "https"):
        return "--prom-url scheme must be http or https"
    if not parts.hostname or port is None:
        return "--prom-url must have a host and a port"
    if parts.path not in ("", "/") or parts.query or parts.fragment:
        return "--prom-url must be the server root, without path, query or fragment"
    return None


def agent(home: Path, prom_url: str) -> dict[str, object]:
    """The whole agent definition: these keys and nothing else."""
    log = str(home / ".local/state/dash-core/dash-core.log")
    return {
        "Label": LABEL,
        # The symlink `current` selects the build; rollback re-points it (README).
        "ProgramArguments": [str(home / ".local/share/dash-core/current/dash-core")],
        # DASH_REKTSLUG_URL is left unset on purpose: the rektslug sheet reads missing.
        "EnvironmentVariables": {"DASH_BIND": BIND, "DASH_PROM_URL": prom_url},
        "RunAtLoad": True,
        "KeepAlive": True,
        # dash-core logs JSON on stdout.
        "StandardOutPath": log,
        "StandardErrorPath": log,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Write the dash-core LaunchAgent plist.")
    parser.add_argument(
        "--prom-url", required=True, help="http://<WG IP of the Prometheus node>:30090"
    )
    args = parser.parse_args()
    home = os.environ.get("HOME", "")
    if not home or not Path(home).is_absolute():
        print("render-plist: refused: HOME is not an absolute path", file=sys.stderr)
        return 1
    error = prom_url_error(args.prom_url)
    if error:
        print(f"render-plist: refused: {error}", file=sys.stderr)
        return 1
    sys.stdout.buffer.write(plistlib.dumps(agent(Path(home), args.prom_url)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
