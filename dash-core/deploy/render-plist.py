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
from collections.abc import Iterator
from pathlib import Path
from typing import cast
from urllib.parse import urlsplit

BINARY = "/Users/{user}/.local/share/dash-core/current/dash-core"


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


def strings(value: object) -> Iterator[str]:
    """Every dict key and string value in a parsed plist."""
    if isinstance(value, str):
        yield value
    elif isinstance(value, list):
        for item in cast(list[object], value):
            yield from strings(item)
    elif isinstance(value, dict):
        for key, item in cast(dict[str, object], value).items():
            yield key
            yield from strings(item)


def shape_error(plist: dict[str, object], user: str) -> str | None:
    """The bind check only holds if nothing else can set DASH_BIND (rev-fd6-r1 R3)."""
    if plist.get("ProgramArguments") != [BINARY.format(user=user)]:
        return "ProgramArguments must be exactly the dash-core binary"
    env = plist.get("EnvironmentVariables")
    others = {k: v for k, v in env.items() if k != "DASH_BIND"} if isinstance(env, dict) else {}
    rest = {k: v for k, v in plist.items() if k != "EnvironmentVariables"}
    if any("DASH_BIND" in text for text in strings([rest, others])):
        return "DASH_BIND appears outside EnvironmentVariables.DASH_BIND"
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
        return "DASH_BIND is not a loopback ip:port"
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
    error = shape_error(cast(dict[str, object], plist), user) or loopback_bind_error(
        cast(dict[str, object], plist)
    )
    if error:
        print(f"render-plist: refused: {error}", file=sys.stderr)
        return 1
    sys.stdout.buffer.write(plistlib.dumps(plist))
    return 0


if __name__ == "__main__":
    sys.exit(main())
