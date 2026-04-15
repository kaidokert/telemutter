#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent


def run(cmd: list[str], env: dict[str, str] | None = None) -> int:
    print(f"\n==> {' '.join(cmd)}")
    proc = subprocess.run(cmd, cwd=ROOT, env=env)
    return proc.returncode


def main() -> int:
    p = argparse.ArgumentParser(description="Run full Telemutter verification suite.")
    p.add_argument("--skip-rust", action="store_true", help="Skip Rust tests.")
    p.add_argument("--skip-python", action="store_true", help="Skip Python tests.")
    p.add_argument(
        "--skip-python-interop",
        action="store_true",
        help="Skip Rust<->Python UDP interop test (keeps other Rust tests).",
    )
    args = p.parse_args()

    if args.skip_rust and args.skip_python:
        print("Nothing to run (both Rust and Python suites were skipped).")
        return 0

    if not args.skip_rust:
        if args.skip_python_interop:
            rust_cmd = ["cargo", "test", "--workspace", "--", "--skip", "interop_"]
            rc = run(rust_cmd)
        else:
            rc = run(["cargo", "test", "--workspace"])
        if rc != 0:
            return rc

    if not args.skip_python:
        env = os.environ.copy()
        py_path = str(ROOT / "python")
        existing = env.get("PYTHONPATH")
        env["PYTHONPATH"] = py_path if not existing else (py_path + os.pathsep + existing)
        rc = run([sys.executable, "-m", "unittest", "discover", "-s", "python/tests", "-v"], env=env)
        if rc != 0:
            return rc

    print("\nAll requested checks passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
