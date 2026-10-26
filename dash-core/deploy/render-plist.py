#!/usr/bin/env python3
"""Render the dash-core LaunchAgent template without shell or XML substitution.

Adapted from gptcompany/gbg-agents scripts/metrics/render-plist.py: same plistlib walk replacing
@USER@. Fail-closed: the rendered DASH_BIND must be a loopback address, because dash-core has no
auth in phase 1 (gbg-agents#883).
"""

import ipaddress
import os
import plistlib
import sys
from pathlib import Path
from typing import cast


def render(value: object, user: str) -> object:
    if isinstance(value, str):
        return value.replace("@USER@", user)
    if isinstance(value, list):
        return [render(item, user) for item in cast(list[object], value)]
    if isinstance(value, dict):
        return {key: render(item, user) for key, item in cast(dict[str, object], value).items()}
    return value


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
    if len(sys.argv) != 2:
        print("usage: render-plist.py <template.plist.in>", file=sys.stderr)
        return 2
    user = os.environ.get("USER", "")
    if not user:
        print("render-plist: USER is not set", file=sys.stderr)
        return 1
    plist = render(plistlib.loads(Path(sys.argv[1]).read_bytes()), user)
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
