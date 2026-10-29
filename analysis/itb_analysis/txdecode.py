"""Decode legacy, v0 and v1 Solana transactions from wire bytes, and check their signatures.

Independent of the Rust sender: the verifier uses this to re-read every signed probe from the logs and from blocks.
v1 layout (SIMD-0385): version byte 129 | header (3 x u8) | config mask (u32 LE) | lifetime (32) | num instructions (u8)
| num addresses (u8) | addresses | config values (4 bytes per mask bit) | instruction headers (u8, u8, u16 LE)
| payloads | signatures (64 each), signing every byte before the signatures.
"""
from __future__ import annotations

import struct
from dataclasses import dataclass, field

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
COMPUTE_BUDGET = "ComputeBudget111111111111111111111111111111"
SYSTEM = "11111111111111111111111111111111"
V1_VERSION = 129


def b58encode(b: bytes) -> str:
    n = int.from_bytes(b, "big")
    s = ""
    while n:
        n, r = divmod(n, 58)
        s = B58[r] + s
    pad = len(b) - len(b.lstrip(b"\0"))
    return "1" * pad + s


def b58decode(s: str) -> bytes:
    n = 0
    for ch in s:
        n = n * 58 + B58.index(ch)
    body = n.to_bytes((n.bit_length() + 7) // 8, "big") if n else b""
    pad = len(s) - len(s.lstrip("1"))
    return b"\0" * pad + body


def _compact_u16(b: bytes, pos: int) -> tuple[int, int]:
    v = 0
    for i in range(3):
        byte = b[pos]
        pos += 1
        v |= (byte & 0x7F) << (7 * i)
        if not byte & 0x80:
            return v, pos
    raise ValueError("compact-u16 too long")


@dataclass
class Ix:
    program_index: int
    accounts: list[int]
    data: bytes


@dataclass
class Tx:
    format: str  # "legacy" | "v0" | "v1"
    header: tuple[int, int, int]
    accounts: list[str]
    blockhash: str
    instructions: list[Ix]
    signatures: list[bytes]
    message: bytes
    v1_config: dict = field(default_factory=dict)

    @property
    def signature(self) -> str:
        return b58encode(self.signatures[0])

    def signatures_valid(self) -> bool:
        for i, sig in enumerate(self.signatures):
            try:
                Ed25519PublicKey.from_public_bytes(b58decode(self.accounts[i])).verify(sig, self.message)
            except (InvalidSignature, ValueError):
                return False
        return True

    def writable(self) -> list[str]:
        """Writable static accounts (lookup-table accounts of v0 are not included)."""
        n_sig, ro_signed, ro_unsigned = self.header
        out = []
        for i, a in enumerate(self.accounts):
            if i < n_sig:
                if i < n_sig - ro_signed:
                    out.append(a)
            elif i < len(self.accounts) - ro_unsigned:
                out.append(a)
        return out

    def compute_budget(self) -> dict:
        """Requested limits: from the config mask (v1) or from ComputeBudget instructions (legacy/v0)."""
        if self.format == "v1":
            return {
                "cu_limit": self.v1_config.get("cu_limit"),
                "priority_fee": self.v1_config.get("priority_fee", 0),
                "loaded_data": self.v1_config.get("loaded_data"),
                "cu_price_micro": None,
            }
        out = {"cu_limit": None, "cu_price_micro": 0, "loaded_data": None}
        for ix in self.instructions:
            if self.accounts[ix.program_index] != COMPUTE_BUDGET or not ix.data:
                continue
            tag = ix.data[0]
            if tag == 2 and len(ix.data) == 5:
                out["cu_limit"] = struct.unpack("<I", ix.data[1:5])[0]
            elif tag == 3 and len(ix.data) == 9:
                out["cu_price_micro"] = struct.unpack("<Q", ix.data[1:9])[0]
            elif tag == 4 and len(ix.data) == 5:
                out["loaded_data"] = struct.unpack("<I", ix.data[1:5])[0]
        if out["cu_limit"] is not None:
            out["priority_fee"] = -(-out["cu_limit"] * out["cu_price_micro"] // 1_000_000)
        else:
            out["priority_fee"] = None
        return out


def first_signature(b: bytes) -> bytes:
    """The fee payer's signature without a full decode: first in legacy/v0, last-but-n in v1 (signatures trail)."""
    if b and b[0] == V1_VERSION:
        n = b[1]
        end = len(b) - 64 * (n - 1)
        return b[end - 64: end]
    _, pos = _compact_u16(b, 0)
    return b[pos: pos + 64]


def decode(b: bytes) -> Tx:
    if b and b[0] == V1_VERSION:
        return _decode_v1(b)
    return _decode_legacy_or_v0(b)


def _decode_legacy_or_v0(b: bytes) -> Tx:
    nsig, pos = _compact_u16(b, 0)
    sigs = [b[pos + 64 * i: pos + 64 * (i + 1)] for i in range(nsig)]
    pos += 64 * nsig
    msg = b[pos:]
    p = 0
    fmt = "legacy"
    if msg[0] & 0x80:
        if msg[0] != 0x80:
            raise ValueError(f"unsupported message version byte {msg[0]}")
        fmt = "v0"
        p = 1
    header = (msg[p], msg[p + 1], msg[p + 2])
    p += 3
    nacc, p = _compact_u16(msg, p)
    accounts = [b58encode(msg[p + 32 * i: p + 32 * (i + 1)]) for i in range(nacc)]
    p += 32 * nacc
    blockhash = b58encode(msg[p: p + 32])
    p += 32
    nix, p = _compact_u16(msg, p)
    ixs = []
    for _ in range(nix):
        prog = msg[p]
        p += 1
        na, p = _compact_u16(msg, p)
        accts = list(msg[p: p + na])
        p += na
        nd, p = _compact_u16(msg, p)
        ixs.append(Ix(prog, accts, msg[p: p + nd]))
        p += nd
    if fmt == "legacy" and p != len(msg):
        raise ValueError("trailing bytes in legacy message")
    return Tx(fmt, header, accounts, blockhash, ixs, sigs, msg)


def _decode_v1(b: bytes) -> Tx:
    p = 1
    header = (b[1], b[2], b[3])
    p = 4
    (mask,) = struct.unpack_from("<I", b, p)
    p += 4
    blockhash = b58encode(b[p: p + 32])
    p += 32
    nix, nacc = b[p], b[p + 1]
    p += 2
    accounts = [b58encode(b[p + 32 * i: p + 32 * (i + 1)]) for i in range(nacc)]
    p += 32 * nacc
    cfg: dict = {"mask": mask}
    if mask & 0b11 == 0b11:
        cfg["priority_fee"] = struct.unpack_from("<Q", b, p)[0]
        p += 8
    elif mask & 0b11:
        raise ValueError("priority-fee mask needs both bits")
    if mask & 0b100:
        cfg["cu_limit"] = struct.unpack_from("<I", b, p)[0]
        p += 4
    if mask & 0b1000:
        cfg["loaded_data"] = struct.unpack_from("<I", b, p)[0]
        p += 4
    if mask & 0b10000:
        cfg["heap"] = struct.unpack_from("<I", b, p)[0]
        p += 4
    heads = []
    for _ in range(nix):
        prog, na, nd = struct.unpack_from("<BBH", b, p)
        heads.append((prog, na, nd))
        p += 4
    ixs = []
    for prog, na, nd in heads:
        accts = list(b[p: p + na])
        p += na
        ixs.append(Ix(prog, accts, b[p: p + nd]))
        p += nd
    msg = b[:p]
    sigs = [b[p + 64 * i: p + 64 * (i + 1)] for i in range(header[0])]
    p += 64 * header[0]
    if p != len(b):
        raise ValueError("trailing bytes after v1 signatures")
    return Tx("v1", header, accounts, blockhash, ixs, sigs, msg, cfg)
