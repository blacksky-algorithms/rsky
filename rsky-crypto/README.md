# rsky-crypto

Rust crate providing basic cryptographic helpers as needed in [atproto](https://atproto.com).

[![Crate](https://img.shields.io/crates/v/rsky-identity?logo=rust&style=flat-square&logoColor=E05D44&color=E05D44)](https://crates.io/crates/rsky-identity)

This crate implements the two currently supported cryptographic systems:

- P-256 elliptic curve: aka "NIST P-256", aka secp256r1 (note the r), aka prime256v1
- K-256 elliptic curve: aka "NIST K-256", aka secp256k1 (note the k)

The details of cryptography in atproto are described in [the specification](https://atproto.com/specs/cryptography). This includes string encodings, validity of "low-S" signatures, byte representation "compression", hashing, and more.

## License

rsky is released under the [Apache License 2.0](../LICENSE).