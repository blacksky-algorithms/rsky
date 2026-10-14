# rsky-oauth

OAuth provider core for AT Protocol services, written in Rust. It is framework agnostic, so a PDS or any other service can build its OAuth endpoints on top of it.

It covers the pieces every provider needs: JWK and JWT primitives, DPoP proof validation (RFC 9449), client metadata, device flows, and the request, token and store types the endpoints share. Every time-dependent check takes the current time as a parameter, so callers and tests stay deterministic.

rsky-pds, rsky-space-host and rsky-daemon use it.

## License

rsky is released under the [Apache License 2.0](../LICENSE).
