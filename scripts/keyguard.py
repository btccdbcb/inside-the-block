#!/usr/bin/env python3
"""Refuse to commit anything that looks like a Solana secret key or an RPC API key.

Checks the staged content of every added/modified file (or, with --all, every tracked file):
  * a JSON array of 64 integers 0-255 (Solana keypair file format),
  * a base58 string of 87-88 characters (a base58-encoded 64-byte secret key),
  * an API key in a URL query string (api-key=, api_key=, apikey=, token=).
Exit 1 with the file and line of every hit. Stdlib only, so it runs before any tool is installed.
"""
import re
import subprocess
import sys

B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
KEYPAIR_ARRAY = re.compile(r"\[\s*(?:\d{1,3}\s*,\s*){63}\d{1,3}\s*\]")
B58_SECRET = re.compile(r"(?<![%s])[%s]{87,88}(?![%s])" % (B58, B58, B58))
URL_KEY = re.compile(r"[?&](?:api[-_]?key|apikey|token)=[A-Za-z0-9_\-]{8,}", re.IGNORECASE)
ALLOW = "keyguard: allow"  # put on a line to whitelist a deliberate test vector


def staged_files():
    out = subprocess.run(["git", "diff", "--cached", "--name-only", "--diff-filter=ACMR", "-z"],
                         capture_output=True, check=True).stdout
    return [p for p in out.decode().split("\0") if p]


def tracked_files():
    out = subprocess.run(["git", "ls-files", "-z"], capture_output=True, check=True).stdout
    return [p for p in out.decode().split("\0") if p]


def content(path, staged):
    if staged:
        return subprocess.run(["git", "show", f":{path}"], capture_output=True, check=True).stdout
    with open(path, "rb") as f:
        return f.read()


def scan(path, data):
    hits = []
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError:
        return hits
    for m in KEYPAIR_ARRAY.finditer(text):
        values = [int(v) for v in re.findall(r"\d+", m.group(0))]
        if all(0 <= v <= 255 for v in values):
            hits.append((m.start(), "64-byte JSON array (keypair file?)"))
    for m in B58_SECRET.finditer(text):
        hits.append((m.start(), "87-88 char base58 string (secret key?)"))
    for m in URL_KEY.finditer(text):
        hits.append((m.start(), "API key in URL"))
    found = []
    for pos, why in hits:
        line_no = text.count("\n", 0, pos) + 1
        line = text.splitlines()[line_no - 1] if text else ""
        if ALLOW not in line:
            found.append(f"{path}:{line_no}: {why}")
    return found


def main():
    staged = "--all" not in sys.argv
    files = staged_files() if staged else tracked_files()
    problems = []
    for path in files:
        problems += scan(path, content(path, staged))
    if problems:
        print("keyguard: refusing, possible secrets:", file=sys.stderr)
        for p in problems:
            print("  " + p, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
