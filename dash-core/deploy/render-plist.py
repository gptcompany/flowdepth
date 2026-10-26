#!/usr/bin/env python3
"""Render the dash-core LaunchAgent template without shell or XML substitution.

Adapted from gptcompany/gbg-agents scripts/metrics/render-plist.py: same plistlib walk replacing
@USER@, plus @PROM_URL@ from --prom-url. Fail-closed: the rendered DASH_BIND must be a loopback
address, because dash-core has no auth in phase 1 (gbg-agents#883), and the Prometheus URL is
required, with no default: the primary's node changes (monitoring-stack runbook).
"""

import argparse
import ipaddress
import os
import plistlib
import sys
from pathlib import Path
from typing import cast
from urllib.parse import urlsplit


def render(value: object, subs: dict[str, str]) -> object:
    if isinstance(value, str):
        for marker, text in subs.items():
            value = value.replace(marker, text)
        return value
    if isinstance(value, list):
        return [render(item, subs) for item in cast(list[object], value)]
    if isinstance(value, dict):
        return {key: render(item, subs) for key, item in cast(dict[str, object], value).items()}
    return value


def prom_url_error(url: str) -> str | None:
    try:
        parts = urlsplit(url)
        port = parts.port
    except ValueError:
        return f"--prom-url {url!r} is not a URL"
    if parts.scheme not in ("http", "https") or not parts.hostname or port is None:
        return f"--prom-url {url!r} is not an http(s) URL with host and port"
    if parts.path not in ("", "/") or parts.query or parts.fragment:
        return f"--prom-url {url!r} must be the server root, without path"
    # Credentials never belong in an agent definition.
    if parts.username or parts.password:
        return "--prom-url must not carry credentials"
    return None


def loopback_bind_error(plist: dict[str, object]) -> str | None:
    env = plist.get("EnvironmentVariables")
    bind = env.get("DASH_BIND") if isinstance(env, dict) else None
    if not isinstance(bind, str):
        return "EnvironmentVariables.DASH_BIND missing"
    host, sep, port = bind.rpartition(":")
    try:
        ip = ipaddress.ip_address(host.removeprefix("[").removesuffix("]"))
    except ValueError:
        ip = None
    if not sep or not port.isdigit() or ip is None or not ip.is_loopback:
        return f"DASH_BIND {bind!r} is not a loopback ip:port"
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description="Render the dash-core LaunchAgent plist.")
    parser.add_argument(
        "--prom-url", required=True, help="http://<WG IP of the Prometheus node>:30090"
    )
    parser.add_argument("template", type=Path)
    args = parser.parse_args()
    user = os.environ.get("USER", "")
    if not user:
        print("render-plist: USER is not set", file=sys.stderr)
        return 1
    error = prom_url_error(args.prom_url)
    if error:
        print(f"render-plist: refused: {error}", file=sys.stderr)
        return 1
    subs = {"@USER@": user, "@PROM_URL@": args.prom_url}
    plist = render(plistlib.loads(args.template.read_bytes()), subs)
    if not isinstance(plist, dict):
        print("render-plist: template is not a plist dict", file=sys.stderr)
        return 1
    error = loopback_bind_error(cast(dict[str, object], plist))
    if error:
        print(f"render-plist: refused: {error}", file=sys.stderr)
        return 1
    sys.stdout.buffer.write(plistlib.dumps(plist))
    return 0


if __name__ == "__main__":
    sys.exit(main())
