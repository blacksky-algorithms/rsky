# rsky-identity

Rust crate for decentralized identities in [atproto](https://atproto.com) using DIDs and handles

[![Crate](https://img.shields.io/crates/v/rsky-identity?logo=rust&style=flat-square&logoColor=E05D44&color=E05D44)](https://crates.io/crates/rsky-identity)

## Example
```rust
use crate::did::did_resolver::DidResolver;
use crate::handle::HandleResolver;

fn resolve_identity() {
    let did_resolver = DidResolver::new();
    let handle_resolver = HandleResolver::new();
    
    let handle = "blacksky.app";

    let did = handle_resolver.resolve(handle)
        .await
        .context("Expected handle to resolve")?;

    println!("Resolved DID: {:?}", did);

    // Resolve DID document
    let doc = did_resolver.resolve(&did)
        .await
        .context("Failed to resolve DID document")?;

    println!("DID Document: {:?}", doc);

    // Force refresh of DID resolution
    let doc2 = did_resolver.resolve(&did)
        .with_force_refresh(true)
        .await
        .context("Failed to force refresh DID document")?;

    // Resolve Atproto-specific data
    let data = did_resolver.resolve_atproto_data(&did)
        .await
        .context("Failed to resolve Atproto data")?;

    // Validate handle matches
    if data.handle != handle {
        panic!("Invalid handle (did not match DID document)");
    }

    Ok(())
}
```

## License

rsky is released under the [Apache License 2.0](../LICENSE).