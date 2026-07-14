use super::db::get_migrated_db;
use super::helpers::account::{
    self, format_account_status, AccountHelperError, AccountStatus, AvailabilityFlags,
};
use super::helpers::{auth, email_token, invite, password};
use super::*;
use crate::models::models::EmailTokenPurpose;
use lexicon_cid::Cid;
use rsky_lexicon::com::atproto::admin::StatusAttr;
use rsky_lexicon::com::atproto::server::AccountCodes;
use rusqlite::params;
use std::str::FromStr;
use std::sync::Once;

const TEST_CID: &str = "bafkreibjfgx2gprinfvicegelk5kosd6y2frmqpqzwqkg7usac74l3t2v4";

static INIT_ENV: Once = Once::new();

pub(crate) fn init_env() {
    INIT_ENV.call_once(|| {
        let defaults = [
            ("PDS_SERVICE_DID", "did:web:localho.st"),
            (
                "PDS_JWT_KEY_K256_PRIVATE_KEY_HEX",
                "9d5907143471e8f0e8df0f8b9512a8c5377878ee767f18fcf961055ecfc071cd",
            ),
            (
                "PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX",
                "71cfcf4882a6cff494c3d0affadd3858eb3a5838e7b5e15170e696a590a4fa01",
            ),
        ];
        for (key, value) in defaults {
            if std::env::var(key).is_err() {
                std::env::set_var(key, value);
            }
        }
    });
}

pub(crate) async fn test_manager() -> (tempfile::TempDir, AccountManager) {
    init_env();
    let dir = tempfile::tempdir().unwrap();
    let db = get_migrated_db(dir.path().join("account.sqlite"))
        .await
        .unwrap();
    (dir, AccountManager::new(db))
}

fn create_opts(did: &str, handle: &str, invite_code: Option<String>) -> CreateAccountOpts {
    CreateAccountOpts {
        did: did.to_owned(),
        handle: handle.to_owned(),
        email: Some(format!("{handle}@example.com")),
        password: Some("password123".to_owned()),
        repo_cid: Cid::from_str(TEST_CID).unwrap(),
        repo_rev: "3jzfcijpj2z2a".to_owned(),
        invite_code,
        deactivated: None,
    }
}

async fn create_test_account(am: &AccountManager, did: &str, handle: &str) -> (String, String) {
    am.create_account(create_opts(did, handle, None))
        .await
        .unwrap()
}

#[tokio::test]
async fn creates_and_fetches_accounts() {
    let (_dir, am) = test_manager().await;
    let (access, refresh) = create_test_account(&am, "did:plc:alice", "alice.test").await;
    assert!(!access.is_empty());
    assert!(!refresh.is_empty());

    // fetch by did and by handle
    let by_did = am
        .get_account("did:plc:alice", None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(by_did.handle, Some("alice.test".to_owned()));
    let by_handle = am.get_account("alice.test", None).await.unwrap().unwrap();
    assert_eq!(by_handle.did, "did:plc:alice");
    assert_eq!(by_handle.email, Some("alice.test@example.com".to_owned()));

    // fetch by email
    let by_email = am
        .get_account_by_email("ALICE.TEST@example.com", None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(by_email.did, "did:plc:alice");
    assert!(am
        .get_account_by_email("missing@example.com", None)
        .await
        .unwrap()
        .is_none());

    assert!(am.is_account_activated("did:plc:alice").await.unwrap());
    assert!(!am.is_account_activated("did:plc:missing").await.unwrap());

    assert_eq!(
        am.get_did_for_actor("alice.test", None).await.unwrap(),
        Some("did:plc:alice".to_owned())
    );
    assert_eq!(am.get_did_for_actor("nope.test", None).await.unwrap(), None);

    // repo root was recorded and can be updated
    am.update_repo_root(
        "did:plc:alice".to_owned(),
        Cid::from_str(TEST_CID).unwrap(),
        "3jzfcijpj2z2b".to_owned(),
    )
    .await
    .unwrap();

    // duplicate did fails
    let err = am
        .create_account(create_opts("did:plc:alice", "alice2.test", None))
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "UserAlreadyExistsError");
    // duplicate email fails on the account row
    let err = am
        .create_account(CreateAccountOpts {
            email: Some("alice.test@example.com".to_owned()),
            ..create_opts("did:plc:alice3", "alice3.test", None)
        })
        .await
        .unwrap_err();
    assert!(err.to_string().contains("UNIQUE constraint failed"));
}

#[tokio::test]
async fn creates_account_without_credentials_and_deactivated() {
    let (_dir, am) = test_manager().await;
    am.create_account(CreateAccountOpts {
        email: None,
        password: None,
        deactivated: Some(true),
        ..create_opts("did:plc:ghost", "ghost.test", None)
    })
    .await
    .unwrap();

    // hidden by default because it is deactivated
    assert!(am
        .get_account("did:plc:ghost", None)
        .await
        .unwrap()
        .is_none());
    let got = am
        .get_account(
            "did:plc:ghost",
            Some(AvailabilityFlags {
                include_taken_down: None,
                include_deactivated: Some(true),
            }),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(got.deactivated_at.is_some());
    assert!(got.delete_after.is_some());
    assert_eq!(got.email, None);
    assert!(!am.is_account_activated("did:plc:ghost").await.unwrap());
    assert_eq!(
        am.get_account_status("did:plc:ghost").await.unwrap(),
        AccountStatus::Deactivated
    );

    am.activate_account("did:plc:ghost").await.unwrap();
    assert_eq!(
        am.get_account_status("did:plc:ghost").await.unwrap(),
        AccountStatus::Active
    );
}

#[tokio::test]
async fn account_status_and_admin_status() {
    let (_dir, am) = test_manager().await;
    create_test_account(&am, "did:plc:bob", "bob.test").await;

    assert_eq!(
        am.get_account_status("did:plc:bob").await.unwrap(),
        AccountStatus::Active
    );
    assert_eq!(
        am.get_account_status("did:plc:missing").await.unwrap(),
        AccountStatus::Deleted
    );

    let admin_status = am
        .get_account_admin_status("did:plc:bob")
        .await
        .unwrap()
        .unwrap();
    assert!(!admin_status.takedown.applied);
    assert!(!admin_status.deactivated.applied);
    assert!(am
        .get_account_admin_status("did:plc:missing")
        .await
        .unwrap()
        .is_none());

    // takedown with an explicit ref
    am.takedown_account(
        "did:plc:bob",
        StatusAttr {
            applied: true,
            r#ref: Some("mod-action-1".to_owned()),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        am.get_account_status("did:plc:bob").await.unwrap(),
        AccountStatus::Takendown
    );
    let admin_status = am
        .get_account_admin_status("did:plc:bob")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(admin_status.takedown.r#ref, Some("mod-action-1".to_owned()));
    // taken down accounts are hidden by default
    assert!(am.get_account("did:plc:bob", None).await.unwrap().is_none());
    assert!(am
        .get_account(
            "did:plc:bob",
            Some(AvailabilityFlags {
                include_taken_down: Some(true),
                include_deactivated: None,
            }),
        )
        .await
        .unwrap()
        .is_some());

    // takedown without a ref falls back to a timestamp, then reverse it
    am.takedown_account(
        "did:plc:bob",
        StatusAttr {
            applied: true,
            r#ref: None,
        },
    )
    .await
    .unwrap();
    am.takedown_account(
        "did:plc:bob",
        StatusAttr {
            applied: false,
            r#ref: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        am.get_account_status("did:plc:bob").await.unwrap(),
        AccountStatus::Active
    );

    // deactivation with an explicit deleteAfter
    am.deactivate_account("did:plc:bob", Some(rsky_common::now()))
        .await
        .unwrap();
    let admin_status = am
        .get_account_admin_status("did:plc:bob")
        .await
        .unwrap()
        .unwrap();
    assert!(admin_status.deactivated.applied);
    am.activate_account("did:plc:bob").await.unwrap();
}

#[tokio::test]
async fn formats_account_status() {
    assert_eq!(
        format_account_status(None).status,
        Some(AccountStatus::Deleted)
    );
    let base = account::ActorAccount {
        did: "did:plc:x".to_owned(),
        handle: None,
        created_at: rsky_common::now(),
        takedown_ref: None,
        deactivated_at: None,
        delete_after: None,
        email: None,
        invites_disabled: None,
        email_confirmed_at: None,
    };
    assert!(format_account_status(Some(base.clone())).active);
    assert_eq!(
        format_account_status(Some(account::ActorAccount {
            takedown_ref: Some("ref".to_owned()),
            ..base.clone()
        }))
        .status,
        Some(AccountStatus::Takendown)
    );
    assert_eq!(
        format_account_status(Some(account::ActorAccount {
            deactivated_at: Some(rsky_common::now()),
            ..base
        }))
        .status,
        Some(AccountStatus::Deactivated)
    );
}

#[tokio::test]
async fn updates_handles_and_emails() {
    let (_dir, am) = test_manager().await;
    create_test_account(&am, "did:plc:carol", "carol.test").await;
    create_test_account(&am, "did:plc:dan", "dan.test").await;

    am.update_handle("did:plc:carol", "carol2.test")
        .await
        .unwrap();
    assert_eq!(
        am.get_did_for_actor("carol2.test", None).await.unwrap(),
        Some("did:plc:carol".to_owned())
    );
    // taking another account's handle fails
    let err = am
        .update_handle("did:plc:carol", "dan.test")
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "UserAlreadyExistsError");

    am.update_email(UpdateEmailOpts {
        did: "did:plc:carol".to_owned(),
        email: "Carol.New@example.com".to_owned(),
    })
    .await
    .unwrap();
    let got = am
        .get_account("did:plc:carol", None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.email, Some("carol.new@example.com".to_owned()));
    // updating to another account's email fails
    let err = am
        .update_email(UpdateEmailOpts {
            did: "did:plc:carol".to_owned(),
            email: "dan.test@example.com".to_owned(),
        })
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "UserAlreadyExistsError");
}

#[tokio::test]
async fn deletes_accounts() {
    let (_dir, am) = test_manager().await;
    create_test_account(&am, "did:plc:erin", "erin.test").await;
    am.create_email_token("did:plc:erin", EmailTokenPurpose::ConfirmEmail)
        .await
        .unwrap();
    am.delete_account("did:plc:erin").await.unwrap();
    assert!(am
        .get_account(
            "did:plc:erin",
            Some(AvailabilityFlags {
                include_taken_down: Some(true),
                include_deactivated: Some(true),
            }),
        )
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn manages_sessions_and_refresh_tokens() {
    let (_dir, am) = test_manager().await;
    let (_, refresh_jwt) = create_test_account(&am, "did:plc:frank", "frank.test").await;

    // rotating the original refresh token grants a new one
    let refresh_payload = auth::decode_refresh_token(refresh_jwt).unwrap();
    let rotated = am
        .rotate_refresh_token(&refresh_payload.jti)
        .await
        .unwrap()
        .unwrap();
    let rotated_payload = auth::decode_refresh_token(rotated.1.clone()).unwrap();
    assert_ne!(rotated_payload.jti, refresh_payload.jti);

    // reuse within the grace period re-issues the same next token id
    let reused = am
        .rotate_refresh_token(&refresh_payload.jti)
        .await
        .unwrap()
        .unwrap();
    let reused_payload = auth::decode_refresh_token(reused.1).unwrap();
    assert_eq!(reused_payload.jti, rotated_payload.jti);

    // rotating an unknown token yields None
    assert!(am
        .rotate_refresh_token(&"unknown-token-id".to_string())
        .await
        .unwrap()
        .is_none());

    // an expired token cannot be rotated and gets tidied up
    let expired_at = rsky_common::time::from_micros_to_str(1_000_000);
    am.db
        .run({
            let jti = rotated_payload.jti.clone();
            move |conn| {
                conn.execute(
                    "UPDATE refresh_token SET \"expiresAt\" = ?1 WHERE id = ?2",
                    params![expired_at, jti],
                )?;
                Ok(())
            }
        })
        .await
        .unwrap();
    assert!(am
        .rotate_refresh_token(&rotated_payload.jti)
        .await
        .unwrap()
        .is_none());

    // create_session with and without an app password
    let (_, session_refresh) = am
        .create_session("did:plc:frank".to_owned(), None)
        .await
        .unwrap();
    let session_payload = auth::decode_refresh_token(session_refresh).unwrap();
    let stored = auth::get_refresh_token(&session_payload.jti, &am.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.app_password_name, None);
    let (_, app_refresh) = am
        .create_session("did:plc:frank".to_owned(), Some("test app".to_owned()))
        .await
        .unwrap();
    let app_payload = auth::decode_refresh_token(app_refresh).unwrap();
    let stored = auth::get_refresh_token(&app_payload.jti, &am.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.app_password_name, Some("test app".to_owned()));

    // revocation
    assert!(am
        .revoke_refresh_token(session_payload.jti.clone())
        .await
        .unwrap());
    assert!(!am.revoke_refresh_token(session_payload.jti).await.unwrap());
    assert!(auth::revoke_refresh_tokens_by_did("did:plc:frank", &am.db)
        .await
        .unwrap());
    assert!(!auth::revoke_refresh_tokens_by_did("did:plc:frank", &am.db)
        .await
        .unwrap());
}

#[tokio::test]
async fn auth_helper_edge_cases() {
    let (_dir, am) = test_manager().await;
    assert!(!auth::get_refresh_token_id().is_empty());

    // a grace period cannot be added when another refresh has already won
    let payload = auth::RefreshToken {
        scope: crate::auth_verifier::AuthScope::Refresh,
        sub: "did:plc:x".to_owned(),
        exp: jwt_simple::prelude::Duration::from_days(90),
        jti: "token-1".to_owned(),
    };
    auth::store_refresh_token(payload, None, &am.db)
        .await
        .unwrap();
    auth::add_refresh_grace_period(
        auth::RefreshGracePeriodOpts {
            id: "token-1".to_owned(),
            expires_at: rsky_common::now(),
            next_id: "next-a".to_owned(),
        },
        &am.db,
    )
    .await
    .unwrap();
    let err = auth::add_refresh_grace_period(
        auth::RefreshGracePeriodOpts {
            id: "token-1".to_owned(),
            expires_at: rsky_common::now(),
            next_id: "next-b".to_owned(),
        },
        &am.db,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        err.downcast_ref::<auth::AuthHelperError>(),
        Some(auth::AuthHelperError::ConcurrentRefresh)
    ));

    // expired token cleanup only removes stale rows
    auth::delete_expired_refresh_tokens("did:plc:x", rsky_common::now(), &am.db)
        .await
        .unwrap();
    assert!(auth::get_refresh_token("token-1", &am.db)
        .await
        .unwrap()
        .is_none());

    // service jwts look like three dot-separated segments
    let service_jwt = auth::create_service_jwt(auth::ServiceJwtParams {
        iss: "did:web:pds.test".to_owned(),
        aud: "did:web:appview.test".to_owned(),
        exp: None,
        lxm: Some("com.atproto.repo.uploadBlob".to_owned()),
        jti: None,
    })
    .await
    .unwrap();
    assert_eq!(service_jwt.split('.').count(), 3);
}

#[tokio::test]
async fn manages_invites() {
    let (_dir, am) = test_manager().await;
    create_test_account(&am, "did:plc:inviter", "inviter.test").await;

    // admin-created codes
    am.create_invite_codes(
        vec![AccountCodes {
            account: "did:plc:inviter".to_owned(),
            codes: vec!["admin-code-1".to_owned(), "admin-code-2".to_owned()],
        }],
        1,
    )
    .await
    .unwrap();

    // account-created codes
    let created = am
        .create_account_invite_codes("did:plc:inviter", vec!["self-code-1".to_owned()], 1, false)
        .await
        .unwrap();
    assert_eq!(created.len(), 1);
    assert!(!created[0].disabled);
    // creating more than expected fails
    let err = am
        .create_account_invite_codes("did:plc:inviter", vec!["self-code-2".to_owned()], 1, true)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("DuplicateCreate"));

    // the failed create rolled back, leaving the two admin codes and one self code
    let codes = am
        .get_account_invite_codes("did:plc:inviter")
        .await
        .unwrap();
    assert_eq!(codes.len(), 3);

    // an account created with an invite records the use
    am.create_account(create_opts(
        "did:plc:invited",
        "invited.test",
        Some("admin-code-1".to_owned()),
    ))
    .await
    .unwrap();
    let invited_by = am
        .get_invited_by_for_accounts(vec!["did:plc:invited".to_owned()])
        .await
        .unwrap();
    assert_eq!(
        invited_by.get("did:plc:invited").unwrap().code,
        "admin-code-1"
    );
    assert!(am
        .get_invited_by_for_accounts(vec![])
        .await
        .unwrap()
        .is_empty());

    // exhausted codes are rejected
    let err = am
        .create_account(create_opts(
            "did:plc:invited2",
            "invited2.test",
            Some("admin-code-1".to_owned()),
        ))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Not enough uses"));

    // unknown codes are rejected
    let err = am
        .create_account(create_opts(
            "did:plc:invited3",
            "invited3.test",
            Some("no-such-code".to_owned()),
        ))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("None or disabled"));

    // disabled codes are rejected
    am.disable_invite_codes(DisableInviteCodesOpts {
        codes: vec!["admin-code-2".to_owned()],
        accounts: vec![],
    })
    .await
    .unwrap();
    let err = am
        .create_account(create_opts(
            "did:plc:invited4",
            "invited4.test",
            Some("admin-code-2".to_owned()),
        ))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("None or disabled"));

    // disabling by account disables the rest
    am.disable_invite_codes(DisableInviteCodesOpts {
        codes: vec![],
        accounts: vec!["did:plc:inviter".to_owned()],
    })
    .await
    .unwrap();
    let codes = am
        .get_account_invite_codes("did:plc:inviter")
        .await
        .unwrap();
    assert!(codes.iter().all(|code| code.disabled));

    am.set_account_invites_disabled("did:plc:inviter", true)
        .await
        .unwrap();
    let got = am
        .get_account("did:plc:inviter", None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.invites_disabled, Some(1));
    am.set_account_invites_disabled("did:plc:inviter", false)
        .await
        .unwrap();
}

#[tokio::test]
async fn manages_passwords() {
    let (_dir, am) = test_manager().await;
    create_test_account(&am, "did:plc:grace", "grace.test").await;

    assert!(am
        .verify_account_password("did:plc:grace", &"password123".to_owned())
        .await
        .unwrap());
    assert!(!am
        .verify_account_password("did:plc:grace", &"wrong".to_owned())
        .await
        .unwrap());
    assert!(!am
        .verify_account_password("did:plc:missing", &"password123".to_owned())
        .await
        .unwrap());

    // app passwords
    let created = am
        .create_app_password("did:plc:grace".to_owned(), "My App".to_owned())
        .await
        .unwrap();
    assert_eq!(created.password.len(), 19);
    let err = am
        .create_app_password("did:plc:grace".to_owned(), "My App".to_owned())
        .await
        .unwrap_err();
    assert!(err
        .to_string()
        .contains("could not create app-specific password"));

    let listed = am.list_app_passwords("did:plc:grace").await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].0, "My App");

    assert_eq!(
        am.verify_app_password("did:plc:grace", &created.password)
            .await
            .unwrap(),
        Some("My App".to_owned())
    );
    assert_eq!(
        am.verify_app_password("did:plc:grace", "1111-2222-3333-4444")
            .await
            .unwrap(),
        None
    );

    // an app password session gets revoked along with the password
    am.create_session("did:plc:grace".to_owned(), Some("My App".to_owned()))
        .await
        .unwrap();
    am.revoke_app_password("did:plc:grace".to_owned(), "My App".to_owned())
        .await
        .unwrap();
    assert!(am
        .list_app_passwords("did:plc:grace")
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        am.verify_app_password("did:plc:grace", &created.password)
            .await
            .unwrap(),
        None
    );

    // account password reset via email token
    let reset_token = am
        .create_email_token("did:plc:grace", EmailTokenPurpose::ResetPassword)
        .await
        .unwrap();
    am.reset_password(ResetPasswordOpts {
        password: "newpassword456".to_owned(),
        token: reset_token,
    })
    .await
    .unwrap();
    assert!(am
        .verify_account_password("did:plc:grace", &"newpassword456".to_owned())
        .await
        .unwrap());
    assert!(!am
        .verify_account_password("did:plc:grace", &"password123".to_owned())
        .await
        .unwrap());
}

#[tokio::test]
async fn password_hash_helpers() {
    let hash = password::gen_salt_and_hash("secret".to_owned()).unwrap();
    assert!(password::verify(&"secret".to_owned(), &hash).unwrap());
    assert!(!password::verify(&"other".to_owned(), &hash).unwrap());
    assert!(password::verify(&"secret".to_owned(), "not-a-phc-hash").is_err());
    assert!(password::hash_with_salt(&"secret".to_owned(), "!invalid salt!").is_err());
}

#[tokio::test]
async fn manages_email_tokens() {
    let (_dir, am) = test_manager().await;
    create_test_account(&am, "did:plc:henry", "henry.test").await;

    // confirm email flow
    let token = am
        .create_email_token("did:plc:henry", EmailTokenPurpose::ConfirmEmail)
        .await
        .unwrap();
    am.assert_valid_email_token("did:plc:henry", EmailTokenPurpose::ConfirmEmail, &token)
        .await
        .unwrap();
    am.confirm_email(ConfirmEmailOpts {
        did: &"did:plc:henry".to_owned(),
        token: &token,
    })
    .await
    .unwrap();
    let got = am
        .get_account("did:plc:henry", None)
        .await
        .unwrap()
        .unwrap();
    assert!(got.email_confirmed_at.is_some());
    // token was consumed
    assert!(am
        .assert_valid_email_token("did:plc:henry", EmailTokenPurpose::ConfirmEmail, &token)
        .await
        .is_err());

    // updating email clears tokens and the confirmation timestamp
    let token = am
        .create_email_token("did:plc:henry", EmailTokenPurpose::UpdateEmail)
        .await
        .unwrap();
    am.assert_valid_email_token_and_cleanup(
        "did:plc:henry",
        EmailTokenPurpose::UpdateEmail,
        &token,
    )
    .await
    .unwrap();
    am.update_email(UpdateEmailOpts {
        did: "did:plc:henry".to_owned(),
        email: "henry2@example.com".to_owned(),
    })
    .await
    .unwrap();
    let got = am
        .get_account("did:plc:henry", None)
        .await
        .unwrap()
        .unwrap();
    assert!(got.email_confirmed_at.is_none());

    // invalid and expired tokens
    assert!(am
        .assert_valid_email_token("did:plc:henry", EmailTokenPurpose::ConfirmEmail, "BOGUS")
        .await
        .unwrap_err()
        .to_string()
        .contains("Token is invalid"));
    let token = am
        .create_email_token("did:plc:henry", EmailTokenPurpose::ConfirmEmail)
        .await
        .unwrap();
    // creating a second token replaces the first
    let replacement = am
        .create_email_token("did:plc:henry", EmailTokenPurpose::ConfirmEmail)
        .await
        .unwrap();
    assert_ne!(token, replacement);
    assert!(am
        .assert_valid_email_token("did:plc:henry", EmailTokenPurpose::ConfirmEmail, &token)
        .await
        .is_err());

    let old = rsky_common::time::from_micros_to_str(1_000_000);
    am.db
        .run({
            let did = "did:plc:henry".to_owned();
            move |conn| {
                conn.execute(
                    "UPDATE email_token SET \"requestedAt\" = ?1 WHERE did = ?2",
                    params![old, did],
                )?;
                Ok(())
            }
        })
        .await
        .unwrap();
    assert!(am
        .assert_valid_email_token(
            "did:plc:henry",
            EmailTokenPurpose::ConfirmEmail,
            &replacement
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("Token is expired"));
    let err = email_token::assert_valid_token_and_find_did(
        EmailTokenPurpose::ConfirmEmail,
        &replacement,
        None,
        &am.db,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("Token is expired"));
    let err = email_token::assert_valid_token_and_find_did(
        EmailTokenPurpose::ConfirmEmail,
        "BOGUS",
        None,
        &am.db,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("Token is invalid"));
}

#[tokio::test]
async fn account_helper_edge_cases() {
    let (_dir, am) = test_manager().await;
    // unique violation detection
    assert!(!account::is_unique_violation(
        &rusqlite::Error::QueryReturnedNoRows
    ));
    // update_email surfaces non-unique-violation errors as-is
    am.db
        .run(|conn| {
            conn.execute_batch("DROP TABLE account")?;
            Ok(())
        })
        .await
        .unwrap();
    let err = account::update_email("did:plc:x", "x@example.com", &am.db)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("no such table"));
    assert!(!matches!(
        err.downcast_ref::<AccountHelperError>(),
        Some(AccountHelperError::UserAlreadyExistsError)
    ));
}

#[tokio::test]
async fn email_token_row_mapping_rejects_unknown_purpose() {
    let (_dir, am) = test_manager().await;
    let err = am
        .db
        .run(|conn| {
            let row = conn.query_row(
                "SELECT 'bogus_purpose', 'did:plc:x', 'TOKEN', '2023-01-01T00:00:00.000Z'",
                [],
                email_token::email_token_from_row,
            )?;
            Ok(row)
        })
        .await;
    assert!(err.is_err());
}

#[tokio::test]
async fn invite_helper_direct_queries() {
    let (_dir, am) = test_manager().await;
    // uses map is empty when no codes are given
    assert!(invite::get_invite_codes_uses_v2(vec![], &am.db)
        .await
        .unwrap()
        .is_empty());
}
