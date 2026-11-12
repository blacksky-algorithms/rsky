use crate::account_manager::AccountManager;
use crate::actor_store::blobstore::BlobstoreFactory;
use crate::actor_store::space::commit::sign_commit;
use crate::actor_store::ActorStore;
use crate::apis::com::atproto::space::kem::ActorKemKeys;
use crate::apis::com::atproto::space::{
    internal_error, open_local_repo, parse_space_uri, space_error,
};
use crate::apis::ApiError;
use crate::space_auth::{authorize_space_read, SpaceReadAuth};
use crate::space_mac::{
    append_vary_header, compute_response_mac, format_mac_header, MacType, WantMacHeader,
    MAC_HEADER_NAME,
};
use crate::space_scope::SpaceRequest;
use lexicon_cid::Cid;
use rocket::http::Header;
use rocket::response::{self, Responder, Response};
use rocket::{Request, State};
use rsky_space::car::repo_car_bytes;
use rsky_space::star::encode_repo_star;
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;

/// The STAR archive's media type. STAR has no registered one; the reference
/// serves it as opaque bytes.
pub const STAR_CONTENT_TYPE: &str = "application/octet-stream";
/// The legacy CAR archive, still served to a client that asks for it.
pub const CAR_CONTENT_TYPE: &str = "application/vnd.ipld.car";

/// A repo archive response, STAR or CAR, with optional MAC header support.
pub struct CarResponder {
    data: Vec<u8>,
    content_type: &'static str,
    mac: Option<([u8; 32], MacType, u32)>,
}

impl CarResponder {
    pub fn new(data: Vec<u8>) -> Self {
        Self::with_content_type(data, CAR_CONTENT_TYPE)
    }

    pub fn with_content_type(data: Vec<u8>, content_type: &'static str) -> Self {
        Self {
            data,
            content_type,
            mac: None,
        }
    }

    pub fn with_mac(mut self, mac: [u8; 32], mac_type: MacType, version: u32) -> Self {
        self.mac = Some((mac, mac_type, version));
        self
    }
}

impl<'r> Responder<'r, 'static> for CarResponder {
    fn respond_to(self, _req: &'r Request<'_>) -> response::Result<'static> {
        let mut response = Response::build()
            .header(Header::new("content-type", self.content_type))
            .sized_body(self.data.len(), std::io::Cursor::new(self.data))
            .finalize();

        // Always add Vary header for caching: the archive format follows
        // `Accept`, and the MAC follows `Atproto-Space-Want-Mac`.
        response.set_header(Header::new("Vary", append_vary_header(Some("Accept"))));

        // Add MAC header if computed
        if let Some((mac, mac_type, version)) = self.mac {
            response.set_header(Header::new(
                MAC_HEADER_NAME,
                format_mac_header(mac_type, version, &mac),
            ));
        }

        Ok(response)
    }
}

/// Serialize a whole permissioned repo (spec §Repo serialization).
///
/// A STAR archive with the `atproto-space-repo` profile: the unsigned commit as
/// metadata, then every record in strict path order. The commit is vouched for
/// by the MAC header, not a signature. A client that asks for
/// `application/vnd.ipld.car` in `Accept` gets the earlier two-root CAR
/// (signed commit + index) instead.
#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.space.getRepo?<space>&<repo>")]
#[allow(clippy::too_many_arguments)]
pub async fn space_get_repo(
    space: String,
    repo: String,
    auth: SpaceReadAuth,
    want_mac: WantMacHeader,
    accept: Option<&rocket::http::Accept>,
    actor_store: &State<ActorStore>,
    blobstore_factory: &State<BlobstoreFactory>,
    account_manager: AccountManager,
) -> Result<CarResponder, ApiError> {
    let space_id = parse_space_uri(&space)?;
    authorize_space_read(
        &auth,
        &space_id,
        &repo,
        &SpaceRequest::ReadSelf { collection: None },
    )?;
    let reader = open_local_repo(
        actor_store,
        blobstore_factory,
        &account_manager,
        &repo,
        false,
        &auth,
    )
    .await?;
    // The caller is authorized by now, so a repo with no commit in this space
    // -- a member who has not written yet -- is the account's own state, and
    // answers `RepoNotFound`, the error the getRepo lexicon declares. Clients
    // read it as an empty repo; `SpaceNotFound` would read as being refused.
    // A deleted space's repo still answers `SpaceDeleted`.
    let state = match reader
        .space
        .repo_state(&space_id.uri())
        .await
        .map_err(space_error)?
    {
        None => {
            return Err(ApiError::BadRequest(
                "RepoNotFound".to_string(),
                format!("{repo} has no commit in {}", space_id.uri()),
            ))
        }
        Some(state) if state.deleted => {
            return Err(ApiError::BadRequest(
                "SpaceDeleted".to_string(),
                format!("`{}` belongs to a deleted space", space_id.uri()),
            ))
        }
        Some(state) => state,
    };
    let hash = state.hash();
    let rev = state.rev.clone();
    let rows = reader
        .space
        .all_records(&space_id.uri())
        .await
        .map_err(space_error)?;
    let wants_car = accept.is_some_and(|accept| {
        accept
            .media_types()
            .any(|media| media.top() == "application" && media.sub() == "vnd.ipld.car")
    });
    let mut responder = if wants_car {
        let mut entries: BTreeMap<String, Cid> = BTreeMap::new();
        let mut blocks: HashMap<Cid, Vec<u8>> = HashMap::new();
        for row in rows {
            let cid =
                Cid::from_str(&row.cid).map_err(internal_error("stored cid failed to parse"))?;
            entries.insert(format!("{}/{}", row.collection, row.rkey), cid);
            blocks.insert(cid, row.value);
        }
        let keypair = reader
            .keypair()
            .await
            .map_err(internal_error("missing actor keypair"))?;
        let commit = sign_commit(&keypair, &space_id.uri(), &repo, &rev, &hash)
            .map_err(internal_error("commit signing failed"))?;
        let car = repo_car_bytes(&commit, &entries, move |cid| blocks.get(cid).cloned())
            .await
            .map_err(internal_error("car serialization failed"))?;
        CarResponder::new(car)
    } else {
        let records = rows
            .into_iter()
            .map(|row| (format!("{}/{}", row.collection, row.rkey), row.value))
            .collect();
        let star = encode_repo_star(&space_id.uri(), &repo, &rev, &hash, records)
            .map_err(internal_error("star serialization failed"))?;
        CarResponder::with_content_type(star, STAR_CONTENT_TYPE)
    };

    // Compute MAC if requested (MAC authenticates the commit, not the archive)
    if let Some(ref want) = want_mac.0 {
        // Validate MAC type matches method
        if want.mac_type != MacType::Commit {
            return Err(ApiError::BadRequest(
                "MacTypeMismatch".to_string(),
                format!(
                    "getRepo requires type=\"atproto-space-repo\", got {:?}",
                    want.mac_type
                ),
            ));
        }

        // MAC under the author's `#atproto_kem` key; an account that has none
        // yet answers without a MAC.
        let kem_provider = ActorKemKeys(actor_store);
        if let Ok(mac) = compute_response_mac(
            &kem_provider,
            want,
            &repo,
            &space,
            &repo,
            Some(&rev),
            Some(&hash),
            None,
            None,
        )
        .await
        {
            responder = responder.with_mac(mac, want.mac_type, want.version);
        }
    }

    Ok(responder)
}
