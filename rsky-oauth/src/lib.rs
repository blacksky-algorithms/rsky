//! Framework-agnostic OAuth provider core for AT Protocol services.
//!
//! Scope: JOSE JWK/JWT primitives and DPoP proof validation (RFC 9449) only.
//! Provider endpoints (PAR, authorize, token) build on this in a later phase.
//!
//! All time-dependent logic takes `now` (unix seconds) parameters so callers
//! and tests stay deterministic.
//!
//! JWT/DPoP signature verification accepts both low-S and high-S ECDSA
//! signatures on BOTH curves (normalize-before-verify), matching upstream
//! jose behavior for interop with wild OAuth clients; DPoP `jti` replay
//! protection neutralizes signature malleability. This policy is deliberate
//! for OAuth only — atproto commit/credential verification elsewhere stays
//! strict low-S.

pub mod dpop;
pub mod error;
pub mod jwk;
pub mod jwt;

pub use dpop::{DpopManager, DpopNonce, DpopProof, DpopRequest, InMemoryReplayStore, ReplayStore};
pub use error::OAuthError;
pub use jwk::{EcCurve, Jwk, JwkSet};
pub use jwt::{DecodedJwt, JwtClaims, JwtHeader};
