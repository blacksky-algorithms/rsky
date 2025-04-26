use crate::jwk::{
    algorithm_as_string, jwk_algorithms, Audience, JwkError, JwtHeader, JwtPayload, Key, SignedJwt,
    VerifyOptions, VerifyResult,
};
use crate::oauth_provider::oidc::sub::Sub;
use biscuit::jwa::*;
use biscuit::jwk::{
    AlgorithmParameters, EllipticCurve, JWKSet, OctetKeyParameters, PublicKeyUse, JWK,
};
use biscuit::jws::*;
use biscuit::*;
use ring::rsa::{KeyPairComponents, PublicKeyComponents};
use ring::signature;
use ring::signature::KeyPair;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub struct JoseKey {
    jwk: JWK<Empty>,
    secret: Secret,
}

impl Key for JoseKey {
    fn is_private(&self) -> bool {
        match &self.jwk.algorithm {
            AlgorithmParameters::EllipticCurve(_) => false,
            AlgorithmParameters::RSA(_) => false,
            AlgorithmParameters::OctetKey(_) => true,
            AlgorithmParameters::OctetKeyPair(_) => false,
        }
    }

    fn is_symetric(&self) -> bool {
        match &self.jwk.algorithm {
            AlgorithmParameters::EllipticCurve(_) => false,
            AlgorithmParameters::RSA(_) => false,
            AlgorithmParameters::OctetKey(_) => true,
            AlgorithmParameters::OctetKeyPair(_) => false,
        }
    }

    fn private_jwk(&self) -> Option<JWK<Empty>> {
        if self.is_private() {
            Some(self.jwk.clone())
        } else {
            None
        }
    }

    fn public_jwk(&self) -> Option<JWK<Empty>> {
        if self.is_symetric() {
            return None;
        }

        let mut jwk = self.jwk.clone();
        let algorithm = jwk.algorithm.clone();
        match algorithm {
            AlgorithmParameters::EllipticCurve(mut params) => {
                params.d = None;
                jwk.algorithm = AlgorithmParameters::EllipticCurve(params);
            }
            AlgorithmParameters::RSA(mut params) => {
                params.d = None;
                jwk.algorithm = AlgorithmParameters::RSA(params);
            }
            AlgorithmParameters::OctetKey(_params) => return None,
            AlgorithmParameters::OctetKeyPair(mut params) => {
                params.d = None;
            }
        }
        Some(jwk)
    }

    fn bare_jwk(&self) -> Option<JWK<Empty>> {
        unimplemented!()
    }

    fn r#use(&self) -> Option<PublicKeyUse> {
        self.jwk.common.public_key_use.clone()
    }

    fn alg(&self) -> Option<Algorithm> {
        self.jwk.common.algorithm
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

    fn algorithms(&self) -> Vec<Algorithm> {
        match self.secret {
            Secret::None => jwk_algorithms(&self.jwk),
            _ => {
                let mut algs = vec![];
                // Secret::RsaKeyPair(secret) => {}
                // Secret::EcdsaKeyPair(secret) => {
                //
                // }
                // Secret::PublicKey(secret) => {}
                // Secret::RSAModulusExponent { .. } => {}
                algs.push(Algorithm::Signature(SignatureAlgorithm::HS256));
                algs.push(Algorithm::Signature(SignatureAlgorithm::HS384));
                algs.push(Algorithm::Signature(SignatureAlgorithm::HS512));
                algs
            }
        }
    }

    fn create_jwt(
        &self,
        header: JwtHeader,
        payload: JwtPayload,
    ) -> Pin<Box<dyn Future<Output = Result<SignedJwt, JwkError>> + Send + Sync + '_>> {
        let header = header.clone();
        let self_kid = self.jwk.common.key_id.clone();
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

            let claims = ClaimsSet::<JwtPayload> {
                registered: RegisteredClaims {
                    issuer: None,
                    subject: None,
                    audience: None,
                    expiry: None,
                    not_before: None,
                    issued_at: None,
                    id: None,
                },
                private: payload.clone(),
            };
            let algorithm = header.alg.clone().unwrap();
            let decoded_jwt = JWT::<JwtPayload, JwtHeader>::new_decoded(
                jws::Header {
                    registered: RegisteredHeader {
                        algorithm: SignatureAlgorithm::HS256,
                        media_type: header.typ.clone(),
                        content_type: header.cty.clone(),
                        web_key_url: header.jku.clone(),
                        web_key: header.jwk.clone(),
                        key_id: header.kid.clone(),
                        x509_url: header.x5u.clone(),
                        x509_chain: header.x5c.clone(),
                        x509_fingerprint: header.x5t.clone(),
                        critical: header.crit.clone(),
                    },
                    private: header,
                },
                claims.clone(),
            );

            match decoded_jwt.into_encoded(&self.secret) {
                Ok(encoded_jwt) => match encoded_jwt.clone() {
                    jws::Compact::Decoded { .. } => {
                        panic!("Should never happen")
                    }
                    jws::Compact::Encoded(jwt) => {
                        let mut result = "".to_string();
                        for part in jwt.parts {
                            result += part.str();
                            result += ".";
                        }
                        result.remove(result.len() - 1);
                        Ok(SignedJwt::new(result).unwrap())
                    }
                },
                Err(error) => Err(JwkError::JwtCreateError(error.to_string())),
            }
        })
    }

    //TODO Needs to use verify
    fn verify_jwt(
        &self,
        token: SignedJwt,
        options: Option<VerifyOptions>,
    ) -> Pin<Box<dyn Future<Output = Result<VerifyResult, JwkError>> + Send + Sync + '_>> {
        let token = token.clone().val();
        Box::pin(async move {
            let expected_jwt = JWT::<JwtPayload, JwtHeader>::new_encoded(token.as_str());
            let jwk_set = JWKSet {
                keys: vec![self.jwk.clone()],
            };
            return match expected_jwt.decode_with_jwks_ignore_kid(&jwk_set) {
                Ok(decoded_jwt) => {
                    let header = decoded_jwt.header().unwrap();
                    let mut protected_header = header.private.clone();
                    protected_header.alg = Some(algorithm_as_string(Algorithm::Signature(
                        header.registered.algorithm,
                    )));

                    protected_header.typ = header.registered.media_type.clone();
                    protected_header.cty = header.registered.content_type.clone();
                    protected_header.jku = header.registered.web_key_url.clone();
                    protected_header.jwk = header.registered.web_key.clone();
                    protected_header.kid = header.registered.key_id.clone();
                    protected_header.x5u = header.registered.x509_url.clone();
                    protected_header.x5c = header.registered.x509_chain.clone();
                    protected_header.x5t = header.registered.x509_fingerprint.clone();
                    protected_header.crit = header.registered.critical.clone();

                    let claimsset = decoded_jwt.payload().unwrap();
                    let mut payload = claimsset.private.clone();

                    payload.iss = claimsset.registered.issuer.clone();
                    payload.sub = match claimsset.registered.subject.clone() {
                        None => None,
                        Some(subject) => Some(Sub::new(subject).unwrap()),
                    };
                    payload.aud = match claimsset.registered.audience.clone() {
                        None => None,
                        Some(audience) => match audience {
                            SingleOrMultiple::Single(audience) => Some(Audience::Single(audience)),
                            SingleOrMultiple::Multiple(audiences) => {
                                Some(Audience::Multiple(audiences))
                            }
                        },
                    };
                    payload.exp = match claimsset.registered.expiry {
                        None => None,
                        Some(expiry) => Some(expiry.timestamp()),
                    };
                    payload.nbf = match claimsset.registered.not_before {
                        None => None,
                        Some(nbf) => Some(nbf.timestamp()),
                    };
                    payload.iat = match claimsset.registered.issued_at {
                        None => None,
                        Some(iat) => Some(iat.timestamp()),
                    };
                    payload.jti = claimsset.registered.id.clone();

                    if let Some(options) = options {
                        if let Some(subject) = options.subject {}
                        if let Some(audience) = options.audience {}
                        if let Some(typ) = options.typ {
                            if let Some(payload_typ) = &protected_header.typ {
                                if &typ != payload_typ {
                                    return Err(JwkError::JwtVerifyError(
                                        "Invalid typ".to_string(),
                                    ));
                                }
                            } else {
                                return Err(JwkError::JwtVerifyError("Invalid typ".to_string()));
                            }
                        }
                        if let Some(issuer) = options.issuer {
                            if let Some(payload_iss) = &payload.iss {
                                if &issuer.into_inner() != payload_iss {
                                    return Err(JwkError::JwtVerifyError(
                                        "Invalid Issuers".to_string(),
                                    ));
                                }
                            } else {
                                return Err(JwkError::JwtVerifyError(
                                    "Invalid Issuers".to_string(),
                                ));
                            }
                        }
                        if let Some(clock_tolerance) = options.clock_tolerance {}
                        if let Some(current_date) = options.current_date {}
                        if let Some(max_token_age) = options.max_token_age {}
                    }

                    Ok(VerifyResult {
                        payload,
                        protected_header,
                    })
                }
                Err(error) => Err(JwkError::JwtVerifyError(error.to_string())),
            };
        })
    }
}

impl JoseKey {
    //jsonwebtoken does not support encoding keys in their jwks, alternative will be needed down road
    pub async fn from_jwk(jwk: JWK<Empty>, input_kid: Option<String>) -> Self {
        match &jwk.algorithm {
            AlgorithmParameters::EllipticCurve(params) => {
                let secret = Secret::None;
                Self { jwk, secret }
            }
            AlgorithmParameters::RSA(params) => {
                let secret = if params.d.is_some() {
                    let key_pair = signature::RsaKeyPair::from_components(&KeyPairComponents {
                        public_key: PublicKeyComponents {
                            n: params.n.to_bytes_be(),
                            e: params.e.to_bytes_be(),
                        },
                        d: params.d.clone().unwrap().to_bytes_be(),
                        p: params.p.clone().unwrap().to_bytes_be(),
                        q: params.q.clone().unwrap().to_bytes_be(),
                        dP: params.dp.clone().unwrap().to_bytes_be(),
                        dQ: params.dq.clone().unwrap().to_bytes_be(),
                        qInv: params.qi.clone().unwrap().to_bytes_be(),
                    })
                    .unwrap();
                    Secret::RsaKeyPair(Arc::new(key_pair))
                } else {
                    Secret::None
                };
                Self { jwk, secret }
            }
            AlgorithmParameters::OctetKey(params) => {
                let secret = Secret::Bytes(params.value.clone());
                Self { jwk, secret }
            }
            AlgorithmParameters::OctetKeyPair(params) => Self {
                jwk,
                secret: Secret::None,
            },
        }
    }

    pub async fn from_secret(secret: Secret, kid: Option<String>, alg: Option<String>) -> Self {
        let key_secret: Secret;
        let jwk: JWK<Empty>;
        let key_secret = secret.clone();
        match secret {
            Secret::None => {
                panic!("Not valid")
            }
            Secret::Bytes(secret) => {
                let jwk = JWK {
                    common: Default::default(),
                    algorithm: AlgorithmParameters::OctetKey(OctetKeyParameters {
                        key_type: Default::default(),
                        value: secret,
                    }),
                    additional: Empty {},
                };
                return Self {
                    jwk,
                    secret: key_secret,
                };
            }
            Secret::RsaKeyPair(keys) => {
                let public_key = keys.public_key();
            }
            Secret::EcdsaKeyPair(_) => {
                panic!()
            }
            Secret::PublicKey(_) => {
                panic!()
            }
            Secret::RSAModulusExponent { .. } => {
                panic!()
            }
        }
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth_types::OAuthIssuerIdentifier;
    use biscuit::jwk::{
        AlgorithmParameters, CommonParameters, EllipticCurve, EllipticCurveKeyParameters,
        EllipticCurveKeyType, RSAKeyParameters,
    };
    use num_bigint::BigUint;
    use ring::rsa::{KeyPairComponents, PublicKeyComponents};
    use ring::signature::RsaKeyPair;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_create_jwt_rsa() {
        let x = RsaKeyPair::from_components(&KeyPairComponents {
            public_key: PublicKeyComponents {
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
                ])
                .to_bytes_be(),
                e: BigUint::new(vec![65537]).to_bytes_be(),
            },
            d: BigUint::new(vec![
                713032433, 400701404, 3861752269, 1672063644, 3365010676, 3983790198, 2118218649,
                1180059196, 3214193513, 103331652, 3890363798, 149974729, 3621157035, 3968873060,
                2871316584, 4055377082, 3404441811, 2991770705, 1288729501, 2747761153, 3336623437,
                2364731106, 1645984872, 1574081430, 3820298762, 2596433775, 3693531604, 4039342668,
                3035475437, 3285541752, 3070172669, 2361416509, 394294662, 2977738861, 2839890465,
                841230222, 883615744, 114031047, 1313725071, 2810669078, 1097346134, 2647740217,
                2124981186, 1406400018, 1957909244, 3961425321, 3596839919, 2973771986, 615724121,
                3146071647, 471749184, 2647156653, 991511652, 3077695114, 748748083, 354410955,
                2713339034, 932263697, 746803531, 2024924924, 1545546613, 4162159596, 3797483017,
                1602687925,
            ])
            .to_bytes_be(),
            p: BigUint::new(vec![
                1238724091, 318372667, 1355643853, 485701733, 3341746677, 1035832885, 3721261079,
                425089171, 2054479354, 1436899400, 562311849, 4217170837, 2023494776, 842246473,
                1670171010, 3629471803, 2613338008, 1336058667, 3907465950, 1278837722, 301706526,
                1508904813, 84700477, 281588688, 1051290981, 4013685922, 1648080502, 3208306609,
                3216888618, 207366948, 2345408890, 4084776684,
            ])
            .to_bytes_be(),
            q: BigUint::new(vec![
                2896074521, 3807517807, 654326695, 805762229, 302497825, 3687241987, 3756840608,
                1743610977, 2621828332, 419985079, 4047291779, 1029002427, 752954010, 2324424862,
                3992768900, 1440975207, 2944332800, 1547302329, 661218096, 1997018012, 248893995,
                1789089172, 2712859322, 2862464495, 3786711083, 2238161202, 1929865911, 3624669681,
                347922466, 3024873892, 3610141359, 3721907783,
            ])
            .to_bytes_be(),
            dP: BigUint::new(vec![
                1155663421, 4052197930, 2875421595, 3507788494, 2881675167, 838917555, 2601651989,
                459386695, 3873213880, 2254232621, 4242124217, 15709214, 292087504, 1069982818,
                1853923539, 1580521844, 4073993812, 2986647068, 2540273745, 2068123243, 2660839470,
                2352030253, 323625625, 2640279336, 791777172, 531977105, 3029809343, 2356061851,
                4159835023, 1928495702, 1195008431, 462098270,
            ])
            .to_bytes_be(),
            dQ: BigUint::new(vec![
                2218750473, 3313252599, 4264517421, 1492081333, 1067765483, 232198298, 2314112310,
                1187650374, 3740239259, 1635257886, 1103093236, 2491201628, 718947546, 1371343186,
                141466945, 37198959, 835764074, 2453692137, 970482580, 2037297103, 698337642,
                4078896579, 3927675986, 897186496, 2305102129, 417341884, 917323172, 1302381423,
                1775079932, 672472846, 3621814299, 3017359391,
            ])
            .to_bytes_be(),
            qInv: BigUint::new(vec![
                2822788373, 565097292, 169554874, 2338166229, 3171059040, 2497414769, 2887328684,
                1224315260, 1462577079, 612121502, 660433863, 1066956358, 2410265369, 3691215764,
                1134057558, 843539511, 694371854, 2599950644, 1558711302, 2053393907, 1148250800,
                1108939089, 377893761, 1098804084, 1819782402, 3151682353, 3812854953, 1602843789,
                369269593, 2731498344, 2724945700, 455294887,
            ])
            .to_bytes_be(),
        })
        .unwrap();
        let secret = Secret::RsaKeyPair(Arc::new(x));
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
                ..Default::default()
            }),
            additional: Default::default(),
        };
        let jose_key = JoseKey::from_jwk(jwk.clone(), None).await;
        let public_jwk = jose_key.public_jwk().unwrap();
        let header = JwtHeader {
            typ: Some("dpop+jwt".to_string()),
            alg: Some("RS256".to_string()),
            jwk: Some(public_jwk),
            ..Default::default()
        };

        let payload = JwtPayload {
            iss: Some("https://cleanfollow-bsky.pages.dev/client-metadata.json".to_string()),
            iat: Some(1745217238),
            jti: Some("h6mlqbjh4w:24v6qmav6v19u".to_string()),
            htm: Some("POST".to_string()),
            htu: Some("https://pds.ripperoni.com/oauth/par".to_string()),
            ..Default::default()
        };
        let signed_jwt = jose_key.create_jwt(header, payload).await.unwrap();
        let expected = SignedJwt::new("eyJhbGciOiJSUzI1NiIsInR5cCI6ImRwb3Arand0IiwiandrIjp7ImFsZyI6IlJTMjU2Iiwia2lkIjoiMjAxMS0wNC0yOSIsImt0eSI6IlJTQSIsIm4iOiIwdng3YWdvZWJHY1FTdXVQaUxKWFpwdE45bm5kclFtYlhFcHMyYWlBRmJXaE03OExoV3g0Y2JiZkFBdFZUODZ6d3UxUks3YVBGRnh1aERSMUw2dFNvY19CSkVDUGViV0tSWGpCWkNpRlY0bjNva25qaE1zdG42NHRaXzJXLTVKc0dZNEhjNW45eUJYQXJ3bDkzbHF0N19STjV3NkNmMGg0UXlRNXYtNjVZR2pRUjBfRkRXMlF2enFZMzY4UVFNaWNBdGFTcXpzOEtKWmduWWI5YzdkMHpnZEFaSHp1NnFNUXZSTDVoYWpybjFuOTFDYk9wYklTRDA4cU5MeXJka3QtYkZUV2hBSTR2TVFGaDZXZVp1MGZNNGxGZDJOY1J3cjNYUGtzSU5IYVEtR194Qm5pSXFidzBMczFqRjQ0LWNzRkN1ci1rRWdVOGF3YXBKektucURLZ3ciLCJlIjoiQVFBQiJ9LCJhbGciOiJSUzI1NiIsImp3ayI6eyJhbGciOiJSUzI1NiIsImtpZCI6IjIwMTEtMDQtMjkiLCJrdHkiOiJSU0EiLCJuIjoiMHZ4N2Fnb2ViR2NRU3V1UGlMSlhacHROOW5uZHJRbWJYRXBzMmFpQUZiV2hNNzhMaFd4NGNiYmZBQXRWVDg2end1MVJLN2FQRkZ4dWhEUjFMNnRTb2NfQkpFQ1BlYldLUlhqQlpDaUZWNG4zb2tuamhNc3RuNjR0Wl8yVy01SnNHWTRIYzVuOXlCWEFyd2w5M2xxdDdfUk41dzZDZjBoNFF5UTV2LTY1WUdqUVIwX0ZEVzJRdnpxWTM2OFFRTWljQXRhU3F6czhLSlpnblliOWM3ZDB6Z2RBWkh6dTZxTVF2Ukw1aGFqcm4xbjkxQ2JPcGJJU0QwOHFOTHlyZGt0LWJGVFdoQUk0dk1RRmg2V2VadTBmTTRsRmQyTmNSd3IzWFBrc0lOSGFRLUdfeEJuaUlxYncwTHMxakY0NC1jc0ZDdXIta0VnVThhd2FwSnpLbnFES2d3IiwiZSI6IkFRQUIifSwidHlwIjoiZHBvcCtqd3QifQ.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ1MjE3MjM4LCJqdGkiOiJoNm1scWJqaDR3OjI0djZxbWF2NnYxOXUiLCJodG0iOiJQT1NUIiwiaHR1IjoiaHR0cHM6Ly9wZHMucmlwcGVyb25pLmNvbS9vYXV0aC9wYXIifQ.fDmSNO1x6_nWceNWGE2y0Eh-GkXatRJC9gSnVoaxd9H5bWYNMzFceVP4eWZx-oQY8nAQXHXJfVGNBtnRG5GV9WYgTqQuOzVbrIoXUasr_-BBe-f26sBgoYxuMhFDn2hJhsx1kZaJvEmI0vUBs5ZSMdnXBE6MDy2me228lJVVWLWhomw44hATk9uwUjceGLR-qOQRXu-Ba4fS6B0W2mgjTYcRwCnIZPiK0pBl8PgFrEJYyPA0YSrntti0SuKBoHJ8Uw6AQzHKQElUfXt-8mWNv3y_ygzBG0nM_imiNSyT9q-gOIOpH2oqIHaH9qDLgiwR2C5mQnuDAjptGPiuKTxQTQ").unwrap();
        assert_eq!(signed_jwt, expected)
    }

    // #[tokio::test]
    // async fn test_create_access_token() {
    //     let secret = Secret::bytes_from_str("secret");
    //     let jose_key = JoseKey::from_secret(secret, None, None).await;
    //     let header = JwtHeader {
    //         typ: Some("at+jwt".to_string()),
    //         alg: Some("HS256".to_string()),
    //         ..Default::default()
    //     };
    //
    //     let payload = JwtPayload {
    //         scope: Some(OAuthScope::new("").unwrap()),
    //         aud: Some(Audience::Single("".to_string())),
    //         sub: Some(Sub::new("".to_string())),
    //         iat: Some(1),
    //         exp: Some(1),
    //         ..Default::default()
    //     };
    //     let signed_jwt = jose_key.create_jwt(header, payload).await.unwrap();
    //     let expected = SignedJwt::new("eyJhbGciOiJSUzI1NiIsInR5cCI6ImRwb3Arand0IiwiandrIjp7ImFsZyI6IlJTMjU2Iiwia2lkIjoiMjAxMS0wNC0yOSIsImt0eSI6IlJTQSIsIm4iOiIwdng3YWdvZWJHY1FTdXVQaUxKWFpwdE45bm5kclFtYlhFcHMyYWlBRmJXaE03OExoV3g0Y2JiZkFBdFZUODZ6d3UxUks3YVBGRnh1aERSMUw2dFNvY19CSkVDUGViV0tSWGpCWkNpRlY0bjNva25qaE1zdG42NHRaXzJXLTVKc0dZNEhjNW45eUJYQXJ3bDkzbHF0N19STjV3NkNmMGg0UXlRNXYtNjVZR2pRUjBfRkRXMlF2enFZMzY4UVFNaWNBdGFTcXpzOEtKWmduWWI5YzdkMHpnZEFaSHp1NnFNUXZSTDVoYWpybjFuOTFDYk9wYklTRDA4cU5MeXJka3QtYkZUV2hBSTR2TVFGaDZXZVp1MGZNNGxGZDJOY1J3cjNYUGtzSU5IYVEtR194Qm5pSXFidzBMczFqRjQ0LWNzRkN1ci1rRWdVOGF3YXBKektucURLZ3ciLCJlIjoiQVFBQiJ9LCJhbGciOiJSUzI1NiIsImp3ayI6eyJhbGciOiJSUzI1NiIsImtpZCI6IjIwMTEtMDQtMjkiLCJrdHkiOiJSU0EiLCJuIjoiMHZ4N2Fnb2ViR2NRU3V1UGlMSlhacHROOW5uZHJRbWJYRXBzMmFpQUZiV2hNNzhMaFd4NGNiYmZBQXRWVDg2end1MVJLN2FQRkZ4dWhEUjFMNnRTb2NfQkpFQ1BlYldLUlhqQlpDaUZWNG4zb2tuamhNc3RuNjR0Wl8yVy01SnNHWTRIYzVuOXlCWEFyd2w5M2xxdDdfUk41dzZDZjBoNFF5UTV2LTY1WUdqUVIwX0ZEVzJRdnpxWTM2OFFRTWljQXRhU3F6czhLSlpnblliOWM3ZDB6Z2RBWkh6dTZxTVF2Ukw1aGFqcm4xbjkxQ2JPcGJJU0QwOHFOTHlyZGt0LWJGVFdoQUk0dk1RRmg2V2VadTBmTTRsRmQyTmNSd3IzWFBrc0lOSGFRLUdfeEJuaUlxYncwTHMxakY0NC1jc0ZDdXIta0VnVThhd2FwSnpLbnFES2d3IiwiZSI6IkFRQUIifSwidHlwIjoiZHBvcCtqd3QifQ.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ1MjE3MjM4LCJqdGkiOiJoNm1scWJqaDR3OjI0djZxbWF2NnYxOXUiLCJodG0iOiJQT1NUIiwiaHR1IjoiaHR0cHM6Ly9wZHMucmlwcGVyb25pLmNvbS9vYXV0aC9wYXIifQ.fDmSNO1x6_nWceNWGE2y0Eh-GkXatRJC9gSnVoaxd9H5bWYNMzFceVP4eWZx-oQY8nAQXHXJfVGNBtnRG5GV9WYgTqQuOzVbrIoXUasr_-BBe-f26sBgoYxuMhFDn2hJhsx1kZaJvEmI0vUBs5ZSMdnXBE6MDy2me228lJVVWLWhomw44hATk9uwUjceGLR-qOQRXu-Ba4fS6B0W2mgjTYcRwCnIZPiK0pBl8PgFrEJYyPA0YSrntti0SuKBoHJ8Uw6AQzHKQElUfXt-8mWNv3y_ygzBG0nM_imiNSyT9q-gOIOpH2oqIHaH9qDLgiwR2C5mQnuDAjptGPiuKTxQTQ").unwrap();
    //     assert_eq!(signed_jwt, expected)
    // }

    #[tokio::test]
    async fn test_verify_jwt() {
        let jwk = JWK {
            common: CommonParameters {
                public_key_use: None,
                key_operations: None,
                algorithm: Some(Algorithm::Signature(SignatureAlgorithm::ES256)),
                key_id: None,
                x509_url: None,
                x509_chain: None,
                x509_fingerprint: None,
            },
            algorithm: AlgorithmParameters::EllipticCurve(EllipticCurveKeyParameters {
                key_type: EllipticCurveKeyType::EC,
                curve: EllipticCurve::P256,
                x: base64_url::decode("2sqMS9VRWkKFfFg3MlkFNGisY5UFItt3OXwVKm-1cR8").unwrap(),
                y: base64_url::decode("HjDdkM8qbvb7W3VKeQ2flalb9jm7pbK7lR6N8MrZgR4").unwrap(),
                d: None,
            }),
            additional: Empty {},
        };
        let jose_key = JoseKey::from_jwk(jwk.clone(), None).await;
        let token = SignedJwt::new("eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7ImFsZyI6IkVTMjU2IiwiY3J2IjoiUC0yNTYiLCJrdHkiOiJFQyIsIngiOiIyc3FNUzlWUldrS0ZmRmczTWxrRk5HaXNZNVVGSXR0M09Yd1ZLbS0xY1I4IiwieSI6IkhqRGRrTThxYnZiN1czVktlUTJmbGFsYjlqbTdwYks3bFI2TjhNclpnUjQifX0.eyJpc3MiOiJodHRwczovL2NsZWFuZm9sbG93LWJza3kucGFnZXMuZGV2L2NsaWVudC1tZXRhZGF0YS5qc29uIiwiaWF0IjoxNzQ1MjE3MjM4LCJqdGkiOiJoNm1scWJqaDR3OjI0djZxbWF2NnYxOXUiLCJodG0iOiJQT1NUIiwiaHR1IjoiaHR0cHM6Ly9wZHMucmlwcGVyb25pLmNvbS9vYXV0aC9wYXIiLCJub25jZSI6IkxlSlYzVm1yT0tXNmlSS0NreVdmeWZfSnc4X01yY1hPUl96eXhVSk1fWHcifQ.np_tNzTWgDuS23BZg1D7phySazSt1jmWJg51YzunA6rvw8T9d6PG5ljZnPlXdARx1Qf9kDQBtv0VMAYRQWnSIw").unwrap();
        let verify_options = Some(VerifyOptions {
            audience: None,
            clock_tolerance: None,
            issuer: Some(
                OAuthIssuerIdentifier::new(
                    "https://cleanfollow-bsky.pages.dev/client-metadata.json",
                )
                .unwrap(),
            ),
            max_token_age: None,
            subject: None,
            typ: Some("dpop+jwt".to_string()),
            current_date: None,
            required_claims: vec![],
        });
        let verify_result = jose_key.verify_jwt(token, None).await.unwrap();
        let expected = VerifyResult {
            payload: JwtPayload {
                iss: Some("https://cleanfollow-bsky.pages.dev/client-metadata.json".to_string()),
                iat: Some(1745217238),
                jti: Some("h6mlqbjh4w:24v6qmav6v19u".to_string()),
                htu: Some("https://pds.ripperoni.com/oauth/par".to_string()),
                htm: Some("POST".to_string()),
                nonce: Some("LeJV3VmrOKW6iRKCkyWfyf_Jw8_MrcXOR_zyxUJM_Xw".to_string()),
                ..Default::default()
            },
            protected_header: JwtHeader {
                typ: Some("dpop+jwt".to_string()),
                alg: Some("ES256".to_string()),
                jwk: Some(jwk),
                ..Default::default()
            },
        };
        assert_eq!(verify_result, expected)
    }
}
