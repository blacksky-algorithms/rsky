use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub fn verify(
    space: &str,
    did: &str,
    cid: &str,
    exp: i64,
    sig: &str,
    now: i64,
    key: Option<&str>,
) -> bool {
    let Some(key) = key.filter(|key| !key.is_empty()) else {
        return false;
    };
    if exp <= now {
        return false;
    }
    let mut mac = match HmacSha256::new_from_slice(key.as_bytes()) {
        Ok(mac) => mac,
        Err(_) => return false,
    };
    mac.update(format!("{space}\n{did}\n{cid}\n{exp}").as_bytes());
    let Ok(signature) = URL_SAFE_NO_PAD.decode(sig) else {
        return false;
    };
    mac.verify_slice(&signature).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signature(space: &str, did: &str, cid: &str, exp: i64) -> String {
        let mut mac = HmacSha256::new_from_slice(b"secret").unwrap();
        mac.update(format!("{space}\n{did}\n{cid}\n{exp}").as_bytes());
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    }

    #[test]
    fn accepts_valid_signature() {
        assert!(verify(
            "at://did:example:space/space/type/key",
            "did:example:author",
            "bafyblob",
            200,
            &signature(
                "at://did:example:space/space/type/key",
                "did:example:author",
                "bafyblob",
                200
            ),
            100,
            Some("secret"),
        ));
    }

    #[test]
    fn rejects_tampered_fields_and_expiry() {
        let space = "at://did:example:space/space/type/key";
        let did = "did:example:author";
        let cid = "bafyblob";
        let sig = signature(space, did, cid, 200);
        assert!(!verify(
            space,
            did,
            "bafyother",
            200,
            &sig,
            100,
            Some("secret")
        ));
        assert!(!verify(
            "at://did:example:space/space/other/key",
            did,
            cid,
            200,
            &sig,
            100,
            Some("secret")
        ));
        assert!(!verify(space, did, cid, 199, &sig, 100, Some("secret")));
        assert!(!verify(space, did, cid, 200, &sig, 200, Some("secret")));
        assert!(!verify(space, did, cid, 200, &sig, 100, None));
    }
}
