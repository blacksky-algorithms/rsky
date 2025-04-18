use crate::jwk::{
    jwk_algorithms, JwkError, JwtHeader, JwtPayload, Key, SignedJwt, VerifyOptions, VerifyResult,
};
use biscuit::errors::Error;
use biscuit::jwa::{Algorithm, SignatureAlgorithm};
use biscuit::jwk::{AlgorithmParameters, EllipticCurve, JWKSet, PublicKeyUse, JWK};
use biscuit::jws::Secret::RsaKeyPair;
use biscuit::jws::{Compact, RegisteredHeader, Secret};
use biscuit::{
    jws, ClaimPresenceOptions, ClaimsSet, CompactPart, RegisteredClaims, TemporalOptions,
    Validation, ValidationOptions, JWT,
};
use num_bigint::BigUint;
use ring::rsa::{KeyPair, KeyPairComponents, PublicKeyComponents};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub type Jwk = JWK<()>;
pub type JwkSet = JWKSet<()>;

pub struct JoseKey {
    jwk: Jwk,
}

impl Key for JoseKey {
    fn is_private(&self) -> bool {
        match &self.jwk.algorithm {
            AlgorithmParameters::EllipticCurve(ec) => ec.d.is_some(),
            AlgorithmParameters::RSA(rsa) => rsa.d.is_some(),
            AlgorithmParameters::OctetKey(key) => !key.value.is_empty(),
            AlgorithmParameters::OctetKeyPair(key_pair) => key_pair.d.is_some(),
        }
    }

    fn is_symetric(&self) -> bool {
        match &self.jwk.algorithm {
            AlgorithmParameters::EllipticCurve(ec) => false,
            AlgorithmParameters::RSA(rsa) => false,
            AlgorithmParameters::OctetKey(key) => true,
            AlgorithmParameters::OctetKeyPair(key_pair) => false,
        }
    }

    fn private_jwk(&self) -> Option<Jwk> {
        if self.is_private() {
            Some(self.jwk.clone())
        } else {
            None
        }
    }

    fn public_jwk(&self) -> Option<Jwk> {
        if self.is_symetric() {
            return None;
        }

        // let mut jwk = self.jwk.clone();
        Some(self.jwk.clone())
    }

    fn bare_jwk(&self) -> Option<Jwk> {
        unimplemented!()
        // if self.is_symetric() {
        //     return None
        // }
        // let mut bare_jwk = self.jwk.clone();
        // let algorithm = match bare_jwk.algorithm {
        //     AlgorithmParameters::EllipticCurve(_) => {}
        //     AlgorithmParameters::RSA(_) => {}
        //     AlgorithmParameters::OctetKey(_) => {}
        //     AlgorithmParameters::OctetKeyPair(_) => {}
        // };
        // unimplemented!()
    }

    fn r#use(&self) -> Option<PublicKeyUse> {
        self.jwk.common.public_key_use.clone()
    }

    fn alg(&self) -> Option<SignatureAlgorithm> {
        match self.jwk.common.algorithm {
            None => None,
            Some(alg) => match alg {
                Algorithm::Signature(alg) => Some(alg),
                Algorithm::KeyManagement(_) => {
                    panic!()
                }
                Algorithm::ContentEncryption(_) => {
                    panic!()
                }
            },
        }
    }

    fn kid(&self) -> Option<String> {
        self.jwk.common.key_id.clone()
    }

    fn crv(&self) -> Option<EllipticCurve> {
        match &self.jwk.algorithm {
            AlgorithmParameters::EllipticCurve(params) => Some(params.curve.clone()),
            AlgorithmParameters::RSA(_) => None,
            AlgorithmParameters::OctetKey(_) => None,
            AlgorithmParameters::OctetKeyPair(params) => Some(params.curve.clone()),
        }
    }

    fn algorithms(&self) -> Vec<SignatureAlgorithm> {
        jwk_algorithms(&self.jwk)
    }

    fn create_jwt(
        &self,
        header: JwtHeader,
        payload: JwtPayload,
    ) -> Pin<Box<dyn Future<Output = Result<SignedJwt, JwkError>> + Send + Sync>> {
        let header = header.clone();
        let self_kid = self.jwk.common.key_id.clone();

        let secret: Secret = match &self.jwk.algorithm {
            AlgorithmParameters::EllipticCurve(_) => {
                eprintln!("elliptic curve");
                panic!()
            }

            AlgorithmParameters::RSA(x) => {
                let d = x.d.clone().unwrap().to_bytes_be();
                let p = x.p.clone().unwrap().to_bytes_be();
                let q = x.q.clone().unwrap().to_bytes_be();
                let dp = x.dp.clone().unwrap().to_bytes_be();
                let dq = x.dq.clone().unwrap().to_bytes_be();
                let qi = x.qi.clone().unwrap().to_bytes_be();
                let components = KeyPairComponents {
                    public_key: PublicKeyComponents {
                        n: x.n.clone().to_bytes_be(),
                        e: x.e.clone().to_bytes_be(),
                    },
                    d,
                    p,
                    q,
                    dP: dp,
                    dQ: dq,
                    qInv: qi,
                };
                RsaKeyPair(Arc::new(KeyPair::from_components(&components).unwrap()))
            }
            AlgorithmParameters::OctetKey(_) => {
                eprintln!("octetkey");
                panic!()
            }
            AlgorithmParameters::OctetKeyPair(_) => {
                eprintln!("octetkeypair");
                panic!()
            }
        };
        Box::pin(async move {
            if let Some(kid) = header.kid.clone() {
                if let Some(self_kid) = self_kid {
                    if kid != self_kid {
                        return Err(JwkError::JwtCreateError(format!(
                            "Invalid \"kid\" ({kid}) used to sign with key \"{self_kid}\""
                        )));
                    }
                }
            }

            let subject = match payload.sub.clone() {
                None => None,
                Some(sub) => Some(sub.get()),
            };
            let claims = ClaimsSet::<JwtPayload> {
                registered: RegisteredClaims {
                    issuer: payload.iss.clone(),
                    subject,
                    audience: None,
                    expiry: None,
                    not_before: None,
                    issued_at: None,
                    id: None,
                },
                private: payload,
            };
            let mut x_token: Compact<ClaimsSet<JwtPayload>, JwtHeader> = JWT::new_decoded(
                (jws::Header {
                    registered: RegisteredHeader {
                        algorithm: SignatureAlgorithm::RS256,
                        media_type: None,
                        content_type: None,
                        web_key_url: None,
                        web_key: None,
                        key_id: None,
                        x509_url: None,
                        x509_chain: None,
                        x509_fingerprint: None,
                        critical: None,
                    },
                    private: header,
                }),
                claims,
            );

            let signed_jwt = match x_token.into_encoded(&secret) {
                Ok(result) => {
                    println!("{:?}", result);
                    println!("{:?}", result.to_base64().unwrap());

                    result.to_base64().unwrap()
                }
                Err(error) => {
                    eprintln!("{}", error);
                    panic!()
                }
            };
            Ok(SignedJwt::new(signed_jwt.to_string()).unwrap())
        })
    }

    fn verify_jwt(
        &self,
        token: SignedJwt,
        options: Option<VerifyOptions>,
    ) -> Pin<Box<dyn Future<Output = Result<VerifyResult, JwkError>> + Send + Sync>> {
        let validate_options = ValidationOptions {
            claim_presence_options: ClaimPresenceOptions {
                issued_at: Default::default(),
                not_before: Default::default(),
                expiry: Default::default(),
                issuer: Default::default(),
                audience: Default::default(),
                subject: Default::default(),
                id: Default::default(),
            },
            temporal_options: TemporalOptions {
                epsilon: Default::default(),
                now: None,
            },
            issued_at: Validation::Ignored,
            not_before: Default::default(),
            expiry: Default::default(),
            issuer: Default::default(),
            audience: Default::default(),
        };
        let jwk = self.jwk.clone();
        let token: Compact<ClaimsSet<JwtPayload>, JwtHeader> =
            JWT::new_encoded(token.clone().val().as_str());
        Box::pin(async move {
            let set = JWKSet { keys: vec![jwk] };
            let result: Compact<ClaimsSet<JwtPayload>, JwtHeader> =
                match token.decode_with_jwks(&set, Some(SignatureAlgorithm::RS256)) {
                    Ok(res) => res,
                    Err(error) => {
                        eprintln!("{}", error);
                        panic!()
                    }
                };

            let payload: &ClaimsSet<JwtPayload> = result.payload().unwrap();

            Ok(VerifyResult {
                payload: payload.private.clone(),
                protected_header: result.header().unwrap().private.clone(),
            })
            // return match jsonwebtoken::decode::<TokenClaims>(&token, &decoding_key, &options) {
            //     Ok(result) => Ok(VerifyResult {
            //         payload: JwtPayload {
            //             iss: result.claims.iss,
            //             aud: result.claims.aud,
            //             sub: result.claims.sub,
            //             exp: result.claims.exp,
            //             nbf: result.claims.nbf,
            //             iat: result.claims.iat,
            //             jti: result.claims.jti,
            //             htm: result.claims.htm,
            //             htu: result.claims.htu,
            //             ath: result.claims.ath,
            //             acr: result.claims.acr,
            //             azp: result.claims.azp,
            //             amr: result.claims.amr,
            //             ..Default::default()
            //         },
            //         protected_header: result.header,
            //     }),
            //     Err(error) => Err(JwkError::JwtVerifyError(error.to_string())),
            // };
        })
    }
}

impl JoseKey {
    pub async fn from_jwk(jwk: Jwk, input_kid: Option<String>) -> Self {
        Self { jwk }
    }

    pub async fn from_hs256(key_like: Vec<u8>, kid: Option<String>) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jwk::{algorithm_as_string, Audience};
    use crate::oauth_provider::oidc::sub::Sub;
    use biscuit::jwa;
    use biscuit::jwa::Algorithm;
    use biscuit::jwk::{AlgorithmParameters, CommonParameters, KeyOperations, RSAKeyParameters};
    use num_bigint::BigUint;
    use std::collections::HashSet;

    #[tokio::test]
    async fn test_create_jwt() {
        let signing_secret = Secret::Bytes("secret".to_string().into_bytes());
        let jwk = JWK {
            common: CommonParameters {
                algorithm: Some(Algorithm::Signature(jwa::SignatureAlgorithm::RS256)),
                key_id: Some("2011-04-29".to_string()),
                ..Default::default()
            },
            algorithm: AlgorithmParameters::RSA(RSAKeyParameters {
                n: BigUint::new(vec![
                    2661337731, 446995658, 1209332140, 183172752, 955894533, 3140848734, 581365968,
                    3217299938, 3520742369, 1559833632, 1548159735, 2303031139, 1726816051,
                    92775838, 37272772, 1817499268, 2876656510, 1328166076, 2779910671, 4258539214,
                    2834014041, 3172137349, 4008354576, 121660540, 1941402830, 1620936445,
                    993798294, 47616683, 272681116, 983097263, 225284287, 3494334405, 4005126248,
                    1126447551, 2189379704, 4098746126, 3730484719, 3232696701, 2583545877,
                    428738419, 2533069420, 2922211325, 2227907999, 4154608099, 679827337,
                    1165541732, 2407118218, 3485541440, 799756961, 1854157941, 3062830172,
                    3270332715, 1431293619, 3068067851, 2238478449, 2704523019, 2826966453,
                    1548381401, 3719104923, 2605577849, 2293389158, 273345423, 169765991,
                    3539762026,
                ]),
                e: BigUint::new(vec![65537]),
                d: Some(BigUint::new(vec![
                    713032433, 400701404, 3861752269, 1672063644, 3365010676, 3983790198,
                    2118218649, 1180059196, 3214193513, 103331652, 3890363798, 149974729,
                    3621157035, 3968873060, 2871316584, 4055377082, 3404441811, 2991770705,
                    1288729501, 2747761153, 3336623437, 2364731106, 1645984872, 1574081430,
                    3820298762, 2596433775, 3693531604, 4039342668, 3035475437, 3285541752,
                    3070172669, 2361416509, 394294662, 2977738861, 2839890465, 841230222,
                    883615744, 114031047, 1313725071, 2810669078, 1097346134, 2647740217,
                    2124981186, 1406400018, 1957909244, 3961425321, 3596839919, 2973771986,
                    615724121, 3146071647, 471749184, 2647156653, 991511652, 3077695114, 748748083,
                    354410955, 2713339034, 932263697, 746803531, 2024924924, 1545546613,
                    4162159596, 3797483017, 1602687925,
                ])),
                p: Some(BigUint::new(vec![
                    1238724091, 318372667, 1355643853, 485701733, 3341746677, 1035832885,
                    3721261079, 425089171, 2054479354, 1436899400, 562311849, 4217170837,
                    2023494776, 842246473, 1670171010, 3629471803, 2613338008, 1336058667,
                    3907465950, 1278837722, 301706526, 1508904813, 84700477, 281588688, 1051290981,
                    4013685922, 1648080502, 3208306609, 3216888618, 207366948, 2345408890,
                    4084776684,
                ])),
                q: Some(BigUint::new(vec![
                    2896074521, 3807517807, 654326695, 805762229, 302497825, 3687241987,
                    3756840608, 1743610977, 2621828332, 419985079, 4047291779, 1029002427,
                    752954010, 2324424862, 3992768900, 1440975207, 2944332800, 1547302329,
                    661218096, 1997018012, 248893995, 1789089172, 2712859322, 2862464495,
                    3786711083, 2238161202, 1929865911, 3624669681, 347922466, 3024873892,
                    3610141359, 3721907783,
                ])),
                dp: Some(BigUint::new(vec![
                    1155663421, 4052197930, 2875421595, 3507788494, 2881675167, 838917555,
                    2601651989, 459386695, 3873213880, 2254232621, 4242124217, 15709214, 292087504,
                    1069982818, 1853923539, 1580521844, 4073993812, 2986647068, 2540273745,
                    2068123243, 2660839470, 2352030253, 323625625, 2640279336, 791777172,
                    531977105, 3029809343, 2356061851, 4159835023, 1928495702, 1195008431,
                    462098270,
                ])),
                dq: Some(BigUint::new(vec![
                    2218750473, 3313252599, 4264517421, 1492081333, 1067765483, 232198298,
                    2314112310, 1187650374, 3740239259, 1635257886, 1103093236, 2491201628,
                    718947546, 1371343186, 141466945, 37198959, 835764074, 2453692137, 970482580,
                    2037297103, 698337642, 4078896579, 3927675986, 897186496, 2305102129,
                    417341884, 917323172, 1302381423, 1775079932, 672472846, 3621814299,
                    3017359391,
                ])),
                qi: Some(BigUint::new(vec![
                    2822788373, 565097292, 169554874, 2338166229, 3171059040, 2497414769,
                    2887328684, 1224315260, 1462577079, 612121502, 660433863, 1066956358,
                    2410265369, 3691215764, 1134057558, 843539511, 694371854, 2599950644,
                    1558711302, 2053393907, 1148250800, 1108939089, 377893761, 1098804084,
                    1819782402, 3151682353, 3812854953, 1602843789, 369269593, 2731498344,
                    2724945700, 455294887,
                ])),
                ..Default::default()
            }),
            additional: Default::default(),
        };
        let jose_key = JoseKey::from_jwk(jwk, None).await;
        let header = JwtHeader {
            typ: Some("dpop+jwt".to_string()),
            alg: Some(algorithm_as_string(SignatureAlgorithm::RS256)),
            kid: Some(String::from("2011-04-29")),
            ..Default::default()
        };
        let payload = JwtPayload {
            iss: Some("https://dev-ejtl988w.auth0.com/".to_string()),
            aud: Some(Audience::Single("http://helloworld".to_string())),
            sub: Some(Sub::new("gZSyspCY5dI4h1Z3qpwpdb9T4UPdGD5k@clients").unwrap()),
            exp: Some(1572492847),
            iat: Some(1572406447),
            azp: Some("gZSyspCY5dI4h1Z3qpwpdb9T4UPdGD5k".to_string()),
            ..Default::default()
        };
        let signed_jwt = jose_key.create_jwt(header, payload).await.unwrap();

        let expected = SignedJwt::new("").unwrap();
        assert_eq!(signed_jwt, expected)
    }

    #[tokio::test]
    async fn test_verify_jwt() {
        let jwk = JWK {
            common: CommonParameters {
                public_key_use: Some(PublicKeyUse::Signature),
                key_operations: Some(vec![KeyOperations::Sign, KeyOperations::Verify]),
                algorithm: Some(Algorithm::Signature(jwa::SignatureAlgorithm::HS256)),
                key_id: Some("2011-04-29".to_string()),
                x509_url: None,
                x509_chain: None,
                x509_fingerprint: None,
            },
            algorithm: AlgorithmParameters::RSA(RSAKeyParameters {
                n: BigUint::new(vec![
                    2661337731, 446995658, 1209332140, 183172752, 955894533, 3140848734, 581365968,
                    3217299938, 3520742369, 1559833632, 1548159735, 2303031139, 1726816051,
                    92775838, 37272772, 1817499268, 2876656510, 1328166076, 2779910671, 4258539214,
                    2834014041, 3172137349, 4008354576, 121660540, 1941402830, 1620936445,
                    993798294, 47616683, 272681116, 983097263, 225284287, 3494334405, 4005126248,
                    1126447551, 2189379704, 4098746126, 3730484719, 3232696701, 2583545877,
                    428738419, 2533069420, 2922211325, 2227907999, 4154608099, 679827337,
                    1165541732, 2407118218, 3485541440, 799756961, 1854157941, 3062830172,
                    3270332715, 1431293619, 3068067851, 2238478449, 2704523019, 2826966453,
                    1548381401, 3719104923, 2605577849, 2293389158, 273345423, 169765991,
                    3539762026,
                ]),
                e: BigUint::new(vec![65537]),
                d: Some(BigUint::new(vec![
                    713032433, 400701404, 3861752269, 1672063644, 3365010676, 3983790198,
                    2118218649, 1180059196, 3214193513, 103331652, 3890363798, 149974729,
                    3621157035, 3968873060, 2871316584, 4055377082, 3404441811, 2991770705,
                    1288729501, 2747761153, 3336623437, 2364731106, 1645984872, 1574081430,
                    3820298762, 2596433775, 3693531604, 4039342668, 3035475437, 3285541752,
                    3070172669, 2361416509, 394294662, 2977738861, 2839890465, 841230222,
                    883615744, 114031047, 1313725071, 2810669078, 1097346134, 2647740217,
                    2124981186, 1406400018, 1957909244, 3961425321, 3596839919, 2973771986,
                    615724121, 3146071647, 471749184, 2647156653, 991511652, 3077695114, 748748083,
                    354410955, 2713339034, 932263697, 746803531, 2024924924, 1545546613,
                    4162159596, 3797483017, 1602687925,
                ])),
                p: Some(BigUint::new(vec![
                    1238724091, 318372667, 1355643853, 485701733, 3341746677, 1035832885,
                    3721261079, 425089171, 2054479354, 1436899400, 562311849, 4217170837,
                    2023494776, 842246473, 1670171010, 3629471803, 2613338008, 1336058667,
                    3907465950, 1278837722, 301706526, 1508904813, 84700477, 281588688, 1051290981,
                    4013685922, 1648080502, 3208306609, 3216888618, 207366948, 2345408890,
                    4084776684,
                ])),
                q: Some(BigUint::new(vec![
                    2896074521, 3807517807, 654326695, 805762229, 302497825, 3687241987,
                    3756840608, 1743610977, 2621828332, 419985079, 4047291779, 1029002427,
                    752954010, 2324424862, 3992768900, 1440975207, 2944332800, 1547302329,
                    661218096, 1997018012, 248893995, 1789089172, 2712859322, 2862464495,
                    3786711083, 2238161202, 1929865911, 3624669681, 347922466, 3024873892,
                    3610141359, 3721907783,
                ])),
                dp: Some(BigUint::new(vec![
                    1155663421, 4052197930, 2875421595, 3507788494, 2881675167, 838917555,
                    2601651989, 459386695, 3873213880, 2254232621, 4242124217, 15709214, 292087504,
                    1069982818, 1853923539, 1580521844, 4073993812, 2986647068, 2540273745,
                    2068123243, 2660839470, 2352030253, 323625625, 2640279336, 791777172,
                    531977105, 3029809343, 2356061851, 4159835023, 1928495702, 1195008431,
                    462098270,
                ])),
                dq: Some(BigUint::new(vec![
                    2218750473, 3313252599, 4264517421, 1492081333, 1067765483, 232198298,
                    2314112310, 1187650374, 3740239259, 1635257886, 1103093236, 2491201628,
                    718947546, 1371343186, 141466945, 37198959, 835764074, 2453692137, 970482580,
                    2037297103, 698337642, 4078896579, 3927675986, 897186496, 2305102129,
                    417341884, 917323172, 1302381423, 1775079932, 672472846, 3621814299,
                    3017359391,
                ])),
                qi: Some(BigUint::new(vec![
                    2822788373, 565097292, 169554874, 2338166229, 3171059040, 2497414769,
                    2887328684, 1224315260, 1462577079, 612121502, 660433863, 1066956358,
                    2410265369, 3691215764, 1134057558, 843539511, 694371854, 2599950644,
                    1558711302, 2053393907, 1148250800, 1108939089, 377893761, 1098804084,
                    1819782402, 3151682353, 3812854953, 1602843789, 369269593, 2731498344,
                    2724945700, 455294887,
                ])),
                ..Default::default()
            }),
            additional: Default::default(),
        };
        let jose_key = JoseKey::from_jwk(jwk, None).await;
        let token = SignedJwt::new("eyJ0eXAiOiJKV1QiLCJhbGciOiJSUzI1NiIsImtpZCI6Ik5FTXlNRUZDTXpVd01URTFRVE5CT1VGRE1FUTFPRGN6UmprNU56QkdRelk0UVRrMVEwWkVPUSJ9.eyJpc3MiOiJodHRwczovL2Rldi1lanRsOTg4dy5hdXRoMC5jb20vIiwic3ViIjoiZ1pTeXNwQ1k1ZEk0aDFaM3Fwd3BkYjlUNFVQZEdENWtAY2xpZW50cyIsImF1ZCI6Imh0dHA6Ly9oZWxsb3dvcmxkIiwiaWF0IjoxNTcyNDA2NDQ3LCJleHAiOjE1NzI0OTI4NDcsImF6cCI6ImdaU3lzcENZNWRJNGgxWjNxcHdwZGI5VDRVUGRHRDVrIiwiZ3R5IjoiY2xpZW50LWNyZWRlbnRpYWxzIn0.nupgm7iFqSnERq9GxszwBrsYrYfMuSfUGj8tGQlkY3Ksh3o_IDfq1GO5ngHQLZuYPD-8qPIovPBEVomGZCo_jYvsbjmYkalAStmF01TvSoXQgJd09ygZstH0liKsmINStiRE8fTA-yfEIuBYttROizx-cDoxiindbKNIGOsqf6yOxf7ww8DrTBJKYRnHVkAfIK8wm9LRpsaOVzWdC7S3cbhCKvANjT0RTRpAx8b_AOr_UCpOr8paj-xMT9Zc9HVCMZLBfj6OZ6yVvnC9g6q_SlTa--fY9SL5eqy6-q1JGoyK_-BQ_YrCwrRdrjoJsJ8j-XFRFWJX09W3oDuZ990nGA").unwrap();
        let mut x = HashSet::new();
        x.insert("http://helloworld".to_string());
        let options = VerifyOptions {
            audience: None,
            clock_tolerance: None,
            issuer: None,
            max_token_age: None,
            subject: None,
            typ: None,
            current_date: None,
            required_claims: vec![],
        };
        let verify_result = jose_key.verify_jwt(token, Some(options)).await.unwrap();
        let expected = VerifyResult {
            payload: JwtPayload {
                iss: Some("https://dev-ejtl988w.auth0.com/".to_string()),
                aud: Some(Audience::Single("https://helloworld".to_string())),
                sub: Some(Sub::new("gZSyspCY5dI4h1Z3qpwpdb9T4UPdGD5k@clients").unwrap()),
                exp: Some(1572492847),
                iat: Some(1572406447),
                azp: Some("gZSyspCY5dI4h1Z3qpwpdb9T4UPdGD5k".to_string()),
                ..Default::default()
            },
            protected_header: JwtHeader {
                typ: Some("JWT".to_string()),
                cty: None,
                alg: Some(algorithm_as_string(SignatureAlgorithm::RS256)),
                jku: None,
                jwk: None,
                kid: Some(String::from("2011-04-29")),
                x5u: None,
                x5c: None,
                x5t: None,
                x5t_s256: None,
                crit: None,
            },
        };
        assert_eq!(verify_result, expected)
    }
}
