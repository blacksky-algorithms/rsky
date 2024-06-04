use crate::common::ipld::cid_for_cbor;
use crate::common::sign::atproto_sign;
use crate::plc::types::{CompatibleOp, CompatibleOpOrTombstone, Operation, Service, Tombstone};
use anyhow::Result;
use libipld::Cid;
use secp256k1::SecretKey;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct CreateAtprotoUpdateOpOpts {
    pub signing_key: Option<String>,
    pub handle: Option<String>,
    pub pds: Option<String>,
    pub rotation_keys: Option<Vec<String>>,
}

pub async fn update_atproto_key_op(
    last_op: CompatibleOp,
    signer: &SecretKey,
    signing_key: String,
) -> Result<Operation> {
    create_atproto_update_op(
        last_op,
        signer,
        CreateAtprotoUpdateOpOpts {
            signing_key: Some(signing_key),
            handle: None,
            pds: None,
            rotation_keys: None,
        },
    )
    .await
}

pub async fn update_handle_op(
    last_op: CompatibleOp,
    signer: &SecretKey,
    handle: String,
) -> Result<Operation> {
    create_atproto_update_op(
        last_op,
        signer,
        CreateAtprotoUpdateOpOpts {
            signing_key: None,
            handle: Some(handle),
            pds: None,
            rotation_keys: None,
        },
    )
    .await
}

pub async fn update_pds_op(
    last_op: CompatibleOp,
    signer: &SecretKey,
    pds: String,
) -> Result<Operation> {
    create_atproto_update_op(
        last_op,
        signer,
        CreateAtprotoUpdateOpOpts {
            signing_key: None,
            handle: None,
            pds: Some(pds),
            rotation_keys: None,
        },
    )
    .await
}

pub async fn update_rotation_keys_op(
    last_op: CompatibleOp,
    signer: &SecretKey,
    rotation_keys: Vec<String>,
) -> Result<Operation> {
    create_atproto_update_op(
        last_op,
        signer,
        CreateAtprotoUpdateOpOpts {
            signing_key: None,
            handle: None,
            pds: None,
            rotation_keys: Some(rotation_keys),
        },
    )
    .await
}

pub async fn create_atproto_update_op(
    last_op: CompatibleOp,
    signer: &SecretKey,
    opts: CreateAtprotoUpdateOpOpts,
) -> Result<Operation> {
    create_update_op(last_op, signer, |normalized: Operation| -> Operation {
        let mut updated = normalized.clone();
        if let Some(signing_key) = &opts.signing_key {
            _ = updated
                .verification_methods
                .insert("atproto".to_string(), signing_key.clone());
        }
        if let Some(handle) = &opts.handle {
            let formatted = ensure_atproto_prefix(handle.clone());
            let handle_i = normalized
                .also_known_as
                .iter()
                .position(|h| h.starts_with("at://"));
            match handle_i {
                None => {
                    updated.also_known_as =
                        [[formatted].as_slice(), normalized.also_known_as.as_slice()].concat()
                }
                Some(handle_i) => {
                    updated.also_known_as = [
                        &normalized.also_known_as[0..handle_i],
                        [formatted].as_slice(),
                        &normalized.also_known_as[handle_i + 1..],
                    ]
                    .concat()
                }
            }
        }
        if let Some(pds) = &opts.pds {
            let formatted = ensure_http_prefix(pds.clone());
            _ = updated.services.insert(
                "atproto_pds".to_string(),
                Service {
                    r#type: "AtprotoPersonalDataServer".to_string(),
                    endpoint: formatted,
                },
            )
        }
        if let Some(rotation_keys) = &opts.rotation_keys {
            updated.rotation_keys = rotation_keys.clone();
        }
        updated
    })
    .await
}

pub async fn create_update_op<G>(
    last_op: CompatibleOp,
    signer: &SecretKey,
    func: G,
) -> Result<Operation>
where
    G: Fn(Operation) -> Operation,
{
    let prev = cid_for_cbor(&last_op)?;
    // omit sig so it doesn't accidentally make its way into the next operation
    let mut normalized = normalize_op(last_op);
    normalized.sig = None;

    let mut unsigned = func(normalized);
    unsigned.prev = Some(prev.to_string());

    match add_signature(CompatibleOpOrTombstone::Operation(unsigned), signer).await? {
        CompatibleOpOrTombstone::Operation(op) => Ok(op),
        _ => panic!("Enum type changed"),
    }
}

pub async fn tombstone_op(prev: Cid, key: &SecretKey) -> Result<Tombstone> {
    match add_signature(
        CompatibleOpOrTombstone::Tombstone(Tombstone {
            r#type: "plc_tombstone".to_string(),
            prev: prev.to_string(),
            sig: None,
        }),
        key,
    )
    .await?
    {
        CompatibleOpOrTombstone::Tombstone(op) => Ok(op),
        _ => panic!("Enum type changed"),
    }
}

pub async fn sign_operation(op: Operation, key: &SecretKey) -> Result<Operation> {
    match add_signature(CompatibleOpOrTombstone::Operation(op), key).await? {
        CompatibleOpOrTombstone::Operation(op) => Ok(op),
        _ => panic!("Enum type changed"),
    }
}

pub async fn add_signature(
    mut obj: CompatibleOpOrTombstone,
    key: &SecretKey,
) -> Result<CompatibleOpOrTombstone> {
    let sig = atproto_sign(&obj, key)?.to_vec();
    obj.set_sig(base64_url::encode(&sig).replace("=", ""));
    Ok(obj)
}

pub fn normalize_op(op: CompatibleOp) -> Operation {
    match op {
        CompatibleOp::Operation(op) => op,
        CompatibleOp::CreateOpV1(op) => Operation {
            r#type: "plc_operation".to_string(),
            rotation_keys: vec![op.recovery_key, op.signing_key.clone()],
            verification_methods: BTreeMap::from([("atproto".to_string(), op.signing_key)]),
            also_known_as: vec![ensure_atproto_prefix(op.handle)],
            services: BTreeMap::from([(
                "atproto_pds".to_string(),
                Service {
                    r#type: "AtprotoPersonalDataServer".to_string(),
                    endpoint: ensure_http_prefix(op.service),
                },
            )]),
            prev: op.prev,
            sig: op.sig,
        },
    }
}

// Util
// ---------------------------

pub fn ensure_http_prefix(str: String) -> String {
    if str.starts_with("http://") || str.starts_with("https://") {
        return str;
    }
    format!("https://{str}")
}

pub fn ensure_atproto_prefix(str: String) -> String {
    if str.starts_with("at://") {
        return str;
    }
    let stripped = str.replace("http://", "").replace("https://", "");
    format!("at://{stripped}")
}
