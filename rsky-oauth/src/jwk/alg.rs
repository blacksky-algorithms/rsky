use biscuit::jwa::{Algorithm, SignatureAlgorithm};
use biscuit::jwk::{AlgorithmParameters, EllipticCurve, PublicKeyUse, JWK};
use biscuit::Empty;

pub fn jwk_algorithms(jwk: &JWK<Empty>) -> Vec<Algorithm> {
    let mut algs = vec![];

    // Ed25519, Ed448, and secp256k1 always have "alg"
    // OKP always has "use"
    if let Some(alg) = jwk.common.algorithm {
        algs.push(alg);
        return algs;
    }

    match &jwk.algorithm {
        AlgorithmParameters::EllipticCurve(alg_params) => {
            if let Some(public_key_use) = &jwk.common.public_key_use {
                match public_key_use {
                    PublicKeyUse::Signature => {
                        match alg_params.curve {
                            EllipticCurve::P256 => {
                                algs.push(Algorithm::Signature(SignatureAlgorithm::ES256));
                                return algs;
                            }
                            EllipticCurve::P384 => {
                                algs.push(Algorithm::Signature(SignatureAlgorithm::ES256));
                                return algs;
                            }
                            EllipticCurve::P521 => {}
                            EllipticCurve::Curve25519 => {
                                // Always have alg
                                return algs;
                            }
                            _ => {}
                        }
                    }
                    PublicKeyUse::Encryption => {}
                    PublicKeyUse::Other(_) => {}
                }
            }
        }
        AlgorithmParameters::RSA(_) => {
            if let Some(public_key_use) = &jwk.common.public_key_use {
                match public_key_use {
                    PublicKeyUse::Signature => {
                        algs.push(Algorithm::Signature(SignatureAlgorithm::PS256));
                        algs.push(Algorithm::Signature(SignatureAlgorithm::PS384));
                        algs.push(Algorithm::Signature(SignatureAlgorithm::PS512));
                        algs.push(Algorithm::Signature(SignatureAlgorithm::RS256));
                        algs.push(Algorithm::Signature(SignatureAlgorithm::RS384));
                        algs.push(Algorithm::Signature(SignatureAlgorithm::RS512));
                    }
                    PublicKeyUse::Encryption => {}
                    PublicKeyUse::Other(_) => {}
                }
            }
        }
        AlgorithmParameters::OctetKey(params) => {
            algs.push(Algorithm::Signature(SignatureAlgorithm::HS256));
            algs.push(Algorithm::Signature(SignatureAlgorithm::HS384));
            algs.push(Algorithm::Signature(SignatureAlgorithm::HS512));
        }
        AlgorithmParameters::OctetKeyPair(params) => {}
    }

    algs
}
