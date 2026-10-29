#!/usr/bin/env python3
"""Refuse to commit anything that looks like a Solana secret key or an RPC API key.

Checks the staged content of every added/modified file (or, with --all, every tracked file; .gz files decompressed):
  * a JSON array of 64 integers 0-255 (Solana keypair file format),
  * a base58 string of 87-88 characters that decodes to a keypair (64 bytes whose second half is the ed25519 public
    key of the first half). Transaction signatures have the same length and alphabet, but no signature has this
    property, so the data files can hold signatures and still be checked,
  * an API key in a URL query string (api-key=, api_key=, apikey=, token=).
  * a path under plan/ or notes/, or a planning or notes file (friend_note, TARGET_EXCLUSIONS, WORKING_NOTES, ...).
Exit 1 with the file and line of every hit. Stdlib only, so it runs before any tool is installed.
"""
import gzip
import hashlib
import re
import subprocess
import sys

B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
KEYPAIR_ARRAY = re.compile(r"\[\s*(?:\d{1,3}\s*,\s*){63}\d{1,3}\s*\]")
B58_SECRET = re.compile(r"(?<![%s])[%s]{87,88}(?![%s])" % (B58, B58, B58))
URL_KEY = re.compile(r"[?&](?:api[-_]?key|apikey|token)=[A-Za-z0-9_\-]{8,}", re.IGNORECASE)
PRIVATE_PATH = re.compile(
    r"(^|/)(plan|notes)(/|$)|(^|/)[^/]*(friend_note|TARGET_EXCLUSIONS|WORKING_NOTES|GOAL_PROMPT|DECISIONS)",
    re.IGNORECASE)
ALLOW = "keyguard: allow"  # put on a line to whitelist a deliberate test vector

# ed25519 public key from a 32-byte seed (RFC 8032 section 5.1.5, reference arithmetic).
P = 2**255 - 19
D = -121665 * pow(121666, P - 2, P) % P
GY = 4 * pow(5, P - 2, P) % P
_gx = pow((GY * GY - 1) * pow(D * GY * GY + 1, P - 2, P), (P + 3) // 8, P)
if (_gx * _gx - (GY * GY - 1) * pow(D * GY * GY + 1, P - 2, P)) % P:
    _gx = _gx * pow(2, (P - 1) // 4, P) % P
GX = _gx if _gx % 2 == 0 else P - _gx
G = (GX, GY, 1, GX * GY % P)


def _add(a, b):
    x = (a[1] - a[0]) * (b[1] - b[0]) % P
    y = (a[1] + a[0]) * (b[1] + b[0]) % P
    c = 2 * a[3] * b[3] * D % P
    d = 2 * a[2] * b[2] % P
    e, f, g, h = y - x, d - c, d + c, y + x
    return (e * f, g * h, f * g, e * h)


def public_key(seed: bytes) -> bytes:
    h = hashlib.sha512(seed).digest()
    s = (int.from_bytes(h[:32], "little") & ((1 << 254) - 8)) | (1 << 254)
    q, pt = (0, 1, 1, 0), G
    while s:
        if s & 1:
            q = _add(q, pt)
        pt = _add(pt, pt)
        s >>= 1
    zi = pow(q[2], P - 2, P)
    x, y = q[0] * zi % P, q[1] * zi % P
    return (y | ((x & 1) << 255)).to_bytes(32, "little")


def b58decode(s: str) -> bytes:
    n = 0
    for ch in s:
        n = n * 58 + B58.index(ch)
    body = n.to_bytes((n.bit_length() + 7) // 8, "big")
    return b"\0" * (len(s) - len(s.lstrip("1"))) + body


_seen: dict[str, bool] = {}


def is_keypair_b58(s: str) -> bool:
    if s not in _seen:
        b = b58decode(s)
        _seen[s] = len(b) != 64 or public_key(b[:32]) == b[32:]
    return _seen[s]


def staged_files():
    out = subprocess.run(["git", "diff", "--cached", "--name-only", "--diff-filter=ACMR", "-z"],
                         capture_output=True, check=True).stdout
    return [p for p in out.decode().split("\0") if p]


def tracked_files():
    out = subprocess.run(["git", "ls-files", "-z"], capture_output=True, check=True).stdout
    return [p for p in out.decode().split("\0") if p]


def content(path, staged):
    if staged:
        data = subprocess.run(["git", "show", f":{path}"], capture_output=True, check=True).stdout
    else:
        with open(path, "rb") as f:
            data = f.read()
    if path.endswith(".gz"):
        try:
            data = gzip.decompress(data)
        except OSError:
            pass
    return data


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
        if is_keypair_b58(m.group(0)):
            hits.append((m.start(), "87-88 char base58 keypair (secret key)"))
    for m in URL_KEY.finditer(text):
        hits.append((m.start(), "API key in URL"))
    found = []
    for pos, why in hits:
        line_no = text.count("\n", 0, pos) + 1
        line = text.splitlines()[line_no - 1] if text else ""
        if ALLOW not in line:
            found.append(f"{path}:{line_no}: {why}")
    return found


def private_paths(files):
    return [f"{p}: private planning or notes file" for p in files if PRIVATE_PATH.search(p)]


def self_test():
    """RFC 8032 test 1 key, a keypair built from it in both file formats, and a real signature (must pass)."""
    seed = bytes.fromhex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")
    pub = public_key(seed)
    assert pub.hex() == "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"
    n, b58 = int.from_bytes(seed + pub, "big"), ""
    while n:
        n, r = divmod(n, 58)
        b58 = B58[r] + b58
    array = "[" + ",".join(str(b) for b in seed + pub) + "]"
    sig = "5K3VfiA9sy95563WRNWsdjCYoVG52u6qyDert8ewJT1oiexfHQsxAFCGsB97cUX2GhCii1r9RBxuG7cPmE5Zak1M"
    assert len(scan("t", b58.encode())) == 1, "base58 keypair not caught"
    assert len(scan("t", array.encode())) == 1, "keypair array not caught"
    assert len(scan("t", sig.encode())) == 0, "signature flagged"
    assert private_paths(["plan/INDEX.md", "docs/friend_note.md", "notes/a.md", "docs/DESIGN.md"]) == \
        ["plan/INDEX.md: private planning or notes file", "docs/friend_note.md: private planning or notes file",
         "notes/a.md: private planning or notes file"], "private path check wrong"
    print("keyguard self-test ok")
    return 0


def main():
    if "--self-test" in sys.argv:
        return self_test()
    staged = "--all" not in sys.argv
    files = staged_files() if staged else tracked_files()
    problems = private_paths(files)
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
