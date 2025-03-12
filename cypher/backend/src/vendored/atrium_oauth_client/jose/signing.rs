use super::Header;
use super::jwt::Claims;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ecdsa::{
    Signature, SignatureSize, SigningKey,
    hazmat::{DigestPrimitive, SignPrimitive},
    signature::Signer,
};
use elliptic_curve::{
    CurveArithmetic, PrimeCurve, Scalar, generic_array::ArrayLength, ops::Invert, subtle::CtOption,
};

pub fn create_signed_jwt<C>(
    key: SigningKey<C>,
    header: Header,
    claims: Claims,
) -> serde_json::Result<String>
where
    C: PrimeCurve + CurveArithmetic + DigestPrimitive,
    Scalar<C>: Invert<Output = CtOption<Scalar<C>>> + SignPrimitive<C>,
    SignatureSize<C>: ArrayLength<u8>,
{
    let header = URL_SAFE_NO_PAD.encode(serde_json::to_string(&header)?);
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_string(&claims)?);
    let signature: Signature<_> = key.sign(format!("{header}.{payload}").as_bytes());
    Ok(format!(
        "{header}.{payload}.{}",
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    ))
}
