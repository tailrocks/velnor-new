# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
"""Verify a minisign signature with the Python standard library only.

Used by scripts/mise-binary-digests.sh to check SHASUMS256.txt against the
pinned mise release key (scripts/mise-minisign.pub) before any digest is
trusted. Only the format mise publishes is accepted: prehashed signatures
("ED": Ed25519 over the BLAKE2b-512 of the file) from an "Ed" public key,
plus the global signature over the trusted comment. Anything else is refused.

The Ed25519 arithmetic is the verification half of the RFC 8032 reference
implementation, with the strict checks libsodium (and so minisign) makes: a
scalar below the group order, canonical point encodings, and no small-order
public key or R.
"""

import base64
import binascii
import hashlib

P = 2**255 - 19
L = 2**252 + 27742317777372353535851937790883648493
D = -121665 * pow(121666, P - 2, P) % P
SQRT_M1 = pow(2, (P - 1) // 4, P)
IDENTITY = (0, 1, 1, 0)


class SignatureError(Exception):
    """The signature, the public key or their encoding is not acceptable."""


def _add(a, b):
    x1, y1, z1, t1 = a
    x2, y2, z2, t2 = b
    pa, pb = (y1 - x1) * (y2 - x2) % P, (y1 + x1) * (y2 + x2) % P
    pc, pd = 2 * t1 * t2 * D % P, 2 * z1 * z2 % P
    e, f, g, h = pb - pa, pd - pc, pd + pc, pb + pa
    return (e * f % P, g * h % P, f * g % P, e * h % P)


def _mul(scalar, point):
    result = IDENTITY
    while scalar:
        if scalar & 1:
            result = _add(result, point)
        point = _add(point, point)
        scalar >>= 1
    return result


def _equal(a, b):
    return (a[0] * b[2] - b[0] * a[2]) % P == 0 and (a[1] * b[2] - b[1] * a[2]) % P == 0


def _decode(data):
    """The point a 32-byte encoding names, or None when it is not canonical and on the curve."""
    y = int.from_bytes(data, "little")
    sign = y >> 255
    y &= (1 << 255) - 1
    if y >= P:
        return None
    x2 = (y * y - 1) * pow(D * y * y + 1, P - 2, P) % P
    if x2 == 0:
        return None if sign else (0, y, 1, 0)
    x = pow(x2, (P + 3) // 8, P)
    if (x * x - x2) % P:
        x = x * SQRT_M1 % P
    if (x * x - x2) % P:
        return None
    if x & 1 != sign:
        x = P - x
    return (x, y, 1, x * y % P)


def _small_order(point):
    return _equal(_mul(8, point), IDENTITY)


_BASE_Y = 4 * pow(5, P - 2, P) % P
BASE = _decode(_BASE_Y.to_bytes(32, "little"))


def ed25519_verify(public_key, message, signature):
    """True when `signature` is a valid Ed25519 signature of `message`."""
    if len(public_key) != 32 or len(signature) != 64:
        return False
    a_point, r_point = _decode(public_key), _decode(signature[:32])
    s = int.from_bytes(signature[32:], "little")
    if a_point is None or r_point is None or s >= L:
        return False
    if _small_order(a_point) or _small_order(r_point):
        return False
    h = int.from_bytes(hashlib.sha512(signature[:32] + public_key + message).digest(), "little") % L
    return _equal(_mul(s, BASE), _add(r_point, _mul(h, a_point)))


def _b64(line, length, what):
    try:
        data = base64.b64decode(line.strip(), validate=True)
    except (binascii.Error, ValueError):
        raise SignatureError(f"{what} is not base64") from None
    if len(data) != length:
        raise SignatureError(f"{what} is {len(data)} bytes, expected {length}")
    return data


def parse_public_key(text):
    """The (key id, 32-byte key) of a minisign public key file."""
    lines = text.splitlines()
    if len(lines) < 2:
        raise SignatureError("public key file has no key line")
    raw = _b64(lines[1], 42, "public key")
    if raw[:2] != b"Ed":
        raise SignatureError("public key is not an Ed25519 minisign key")
    return raw[2:10], raw[10:]


def verify(public_key_text, signature_text, message):
    """Return the signed trusted comment, or raise SignatureError.

    `message` is the signed file's content as bytes. The function never returns
    for a file the key did not sign.
    """
    key_id, key = parse_public_key(public_key_text)
    lines = signature_text.splitlines()
    if len(lines) < 4 or not lines[2].startswith("trusted comment: "):
        raise SignatureError("not a minisign signature file")
    raw = _b64(lines[1], 74, "signature")
    if raw[:2] != b"ED":
        raise SignatureError("signature is not a prehashed minisign signature (ED)")
    if raw[2:10] != key_id:
        raise SignatureError("signature was made by a different key than the pinned one")
    signature = raw[10:]
    if not ed25519_verify(key, hashlib.blake2b(message).digest(), signature):
        raise SignatureError("signature does not match the file")
    trusted = lines[2][len("trusted comment: "):]
    global_signature = _b64(lines[3], 64, "global signature")
    if not ed25519_verify(key, signature + trusted.encode(), global_signature):
        raise SignatureError("global signature does not match the trusted comment")
    return trusted
