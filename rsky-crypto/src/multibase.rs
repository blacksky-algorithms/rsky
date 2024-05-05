use anyhow::{bail, Result};
use multibase::{encode, Base};

pub fn multibase_to_bytes(mb: String) -> Result<Vec<u8>> {
    match mb.get(0..1) {
        None => bail!("empty multibase string"),
        Some(base) => match (base, mb.get(1..)) {
            ("f", Some(key)) => Ok(encode(Base::Base16Lower, key).into_bytes()),
            ("F", Some(key)) => Ok(encode(Base::Base16Upper, key).into_bytes()),
            ("b", Some(key)) => Ok(encode(Base::Base32Lower, key).into_bytes()),
            ("B", Some(key)) => Ok(encode(Base::Base32Upper, key).into_bytes()),
            ("z", Some(key)) => Ok(encode(Base::Base58Btc, key).into_bytes()),
            ("m", Some(key)) => Ok(encode(Base::Base64, key).into_bytes()),
            ("u", Some(key)) => Ok(encode(Base::Base64Url, key).into_bytes()),
            ("U", Some(key)) => Ok(encode(Base::Base64UrlPad, key).into_bytes()),
            (&_, _) => bail!("Unsupported multibase: {mb}"),
        },
    }
}
