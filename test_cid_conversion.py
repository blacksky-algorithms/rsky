#!/usr/bin/env python3
"""Test CID conversion logic"""

import base64


def bytes_to_cid_v1(byte_array):
    """
    Convert byte array to CIDv1 string.

    CIDv1 format:
    - byte[0]: version (1)
    - byte[1]: codec (0x55 = raw = 85)
    - byte[2]: hash function (0x12 = sha256 = 18)
    - byte[3]: hash length (0x20 = 32)
    - byte[4:36]: hash bytes (32 bytes)
    """
    if len(byte_array) != 36:
        raise ValueError(f"Expected 36 bytes, got {len(byte_array)}")

    version, codec, hash_fn, hash_len = byte_array[:4]

    print(f"CID structure:")
    print(f"  Version: {version} (expect 1)")
    print(f"  Codec: {codec} (0x{codec:02x}, expect 0x55 for raw)")
    print(f"  Hash function: {hash_fn} (0x{hash_fn:02x}, expect 0x12 for sha256)")
    print(f"  Hash length: {hash_len} (expect 32)")

    if version != 1:
        raise ValueError(f"Wrong version: {version}")
    if codec != 0x55:
        raise ValueError(f"Wrong codec: {codec}")
    if hash_fn != 0x12:
        raise ValueError(f"Wrong hash function: {hash_fn}")
    if hash_len != 32:
        raise ValueError(f"Wrong hash length: {hash_len}")

    # For CIDv1 with base32:
    # We need to encode bytes[1:] (skip version byte) as base32
    # CIDv1 base32 encoding uses lowercase RFC 4648 without padding

    cid_bytes = bytes(byte_array[1:])  # Skip version byte

    # Encode as base32 (RFC 4648)
    encoded = base64.b32encode(cid_bytes).decode('ascii')

    # Convert to lowercase and remove padding
    encoded = encoded.lower().rstrip('=')

    # Add multibase prefix 'b' for base32
    cid_string = 'b' + encoded

    return cid_string


# Test with example from database
test_bytes = [1, 85, 18, 32, 53, 200, 170, 252, 248, 164, 102, 188, 130, 25, 215, 52, 203, 146, 215, 60, 77, 125, 126, 70, 180, 46, 207, 17, 225, 206, 211, 81, 108, 209, 83, 250]

print(f"Input bytes ({len(test_bytes)}): {test_bytes[:10]}...{test_bytes[-5:]}")
print()

try:
    cid = bytes_to_cid_v1(test_bytes)
    print(f"\n✓ SUCCESS!")
    print(f"CID: {cid}")
    print(f"Length: {len(cid)}")
except Exception as e:
    print(f"\n✗ FAILED: {e}")

# Also test with the known good CID from our test post
print("\n" + "="*60)
print("Reverse test: decode a known good CID")
print("="*60)

known_good_cid = "bafkreiddj2ctjilffc4zupzkp3247mfq6hirce6uy5nn2kgk7zbtb75ykm"
print(f"Known good CID: {known_good_cid}")

# Remove the 'b' prefix
if known_good_cid[0] == 'b':
    encoded_part = known_good_cid[1:]
    print(f"Encoded part: {encoded_part}")

    # Decode base32
    # Need to uppercase and add padding
    encoded_upper = encoded_part.upper()
    # Add padding
    padding_needed = (8 - len(encoded_upper) % 8) % 8
    encoded_padded = encoded_upper + '=' * padding_needed

    print(f"Padded for decode: {encoded_padded}")

    decoded_bytes = base64.b32decode(encoded_padded)
    print(f"Decoded bytes: [1, {', '.join(str(b) for b in decoded_bytes[:10])}, ...]")
    print(f"Total bytes: {len(decoded_bytes) + 1} (including version byte)")

    # Show structure
    codec = decoded_bytes[0]
    hash_fn = decoded_bytes[1]
    hash_len = decoded_bytes[2]

    print(f"\nDecoded structure:")
    print(f"  Version: 1 (implied by CIDv1)")
    print(f"  Codec: {codec} (0x{codec:02x})")
    print(f"  Hash function: {hash_fn} (0x{hash_fn:02x})")
    print(f"  Hash length: {hash_len}")
