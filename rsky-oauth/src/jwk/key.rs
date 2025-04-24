use crate::jwk::{JwkError, JwtHeader, JwtPayload, SignedJwt, VerifyOptions, VerifyResult};
use biscuit::jwa::Algorithm;
use biscuit::jwk::{EllipticCurve, PublicKeyUse, JWK};
use biscuit::Empty;
use std::future::Future;
use std::pin::Pin;

pub trait Key: Send + Sync {
    fn is_private(&self) -> bool;

    fn is_symetric(&self) -> bool;

    fn private_jwk(&self) -> Option<JWK<Empty>>;

    fn public_jwk(&self) -> Option<JWK<Empty>>;

    fn bare_jwk(&self) -> Option<JWK<Empty>>;

    fn r#use(&self) -> Option<PublicKeyUse>;

    /**
     * The (forced) algorithm to use. If not provided, the key will be usable with
     * any of the algorithms in {@link algorithms}.
     *
     * @see {@link https://datatracker.ietf.org/doc/html/rfc7518#section-3.1 | "alg" (Algorithm) Header Parameter Values for JWS}
     */
    fn alg(&self) -> Option<Algorithm>;

    fn kid(&self) -> Option<String>;

    fn crv(&self) -> Option<EllipticCurve>;

    fn algorithms(&self) -> Vec<Algorithm>;

    /**
     * Create a signed JWT
     */
    fn create_jwt(
        &self,
        header: JwtHeader,
        payload: JwtPayload,
    ) -> Pin<Box<dyn Future<Output = Result<SignedJwt, JwkError>> + Send + Sync + '_>>;

    /**
     * Verify the signature, headers and payload of a JWT
     *
     * @throws {JwtVerifyError} if the JWT is invalid
     */
    fn verify_jwt(
        &self,
        token: SignedJwt,
        options: Option<VerifyOptions>,
    ) -> Pin<Box<dyn Future<Output = Result<VerifyResult, JwkError>> + Send + Sync + '_>>;
}
