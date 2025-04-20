use jsonwebtoken::jwk::{AlgorithmParameters, EllipticCurve, Jwk, PublicKeyUse};
use jsonwebtoken::Algorithm;
use std::str::FromStr;

pub fn jwk_algorithms(jwk: &Jwk) -> Vec<Algorithm> {
    let mut algs = vec![];

    // Ed25519, Ed448, and secp256k1 always have "alg"
    // OKP always has "use"
    if let Some(alg) = jwk.common.key_algorithm {
        algs.push(Algorithm::from_str(alg.to_string().as_str()).unwrap());
        return algs;
    }

    match &jwk.algorithm {
        AlgorithmParameters::EllipticCurve(alg_params) => {
            if let Some(public_key_use) = &jwk.common.public_key_use {
                match public_key_use {
                    PublicKeyUse::Signature => {
                        match alg_params.curve {
                            EllipticCurve::P256 => {
                                algs.push(Algorithm::ES256);
                                return algs;
                            }
                            EllipticCurve::P384 => {
                                algs.push(Algorithm::ES256);
                                return algs;
                            }
                            EllipticCurve::P521 => {}
                            EllipticCurve::Ed25519 => {
                                // Always have alg
                                return algs;
                            }
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
                        algs.push(Algorithm::PS256);
                        algs.push(Algorithm::PS384);
                        algs.push(Algorithm::PS512);
                        algs.push(Algorithm::RS256);
                        algs.push(Algorithm::RS384);
                        algs.push(Algorithm::RS512);
                    }
                    PublicKeyUse::Encryption => {}
                    PublicKeyUse::Other(_) => {}
                }
            }
        }
        AlgorithmParameters::OctetKey(_) => {}
        AlgorithmParameters::OctetKeyPair(_) => {}
    }

    algs
}
