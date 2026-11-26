"""Print a public key for the extension manifest and the extension ID it produces.

Chrome derives an unpacked extension's ID from the "key" in manifest.json, so a fixed key gives a stable ID, which
the native-messaging host manifest needs in "allowed_origins". Only the PUBLIC key goes in the repo. The private key
isn't needed unless the extension is ever packed as a .crx, so it isn't saved.

    python scripts/extension_key.py
"""
import base64
import hashlib

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import rsa


def extension_id(der: bytes) -> str:
    digest = hashlib.sha256(der).hexdigest()[:32]
    return "".join(chr(ord("a") + int(c, 16)) for c in digest)


if __name__ == "__main__":
    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    der = key.public_key().public_bytes(serialization.Encoding.DER, serialization.PublicFormat.SubjectPublicKeyInfo)
    print("key:", base64.b64encode(der).decode())
    print("id: ", extension_id(der))
