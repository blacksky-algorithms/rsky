use crate::jwk::{JwkError, JwtHeader, JwtPayload, SignedJwt, VerifyOptions, VerifyResult};
use crate::jwk_jose::jose_key::Jwk;
use biscuit::jwa::{Algorithm, SignatureAlgorithm};
use biscuit::jwk::{EllipticCurve, PublicKeyUse};
use std::future::Future;
use std::pin::Pin;

pub trait Key: Send + Sync {
    fn is_private(&self) -> bool;

    fn is_symetric(&self) -> bool;

    fn private_jwk(&self) -> Option<Jwk>;

    fn public_jwk(&self) -> Option<Jwk>;

    fn bare_jwk(&self) -> Option<Jwk>;

    fn r#use(&self) -> Option<PublicKeyUse>;

    /**
     * The (forced) algorithm to use. If not provided, the key will be usable with
     * any of the algorithms in {@link algorithms}.
     *
     * @see {@link https://datatracker.ietf.org/doc/html/rfc7518#section-3.1 | "alg" (Algorithm) Header Parameter Values for JWS}
     */
    fn alg(&self) -> Option<SignatureAlgorithm>;

    fn kid(&self) -> Option<String>;

    fn crv(&self) -> Option<EllipticCurve>;

    fn algorithms(&self) -> Vec<SignatureAlgorithm>;

    /**
     * Create a signed JWT
     */
    fn create_jwt(
        &self,
        header: JwtHeader,
        payload: JwtPayload,
    ) -> Pin<Box<dyn Future<Output = Result<SignedJwt, JwkError>> + Send + Sync>>;

    /**
     * Verify the signature, headers and payload of a JWT
     *
     * @throws {JwtVerifyError} if the JWT is invalid
     */
    fn verify_jwt(
        &self,
        token: SignedJwt,
        options: Option<VerifyOptions>,
    ) -> Pin<Box<dyn Future<Output = Result<VerifyResult, JwkError>> + Send + Sync>>;
}
