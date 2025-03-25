// @generated automatically by Diesel CLI.

pub mod pds {
    diesel::table! {
        pds.account (did) {
            did -> Varchar,
            email -> Varchar,
            recoveryKey -> Nullable<Varchar>,
            password -> Varchar,
            createdAt -> Varchar,
            invitesDisabled -> Int2,
            emailConfirmedAt -> Nullable<Varchar>,
        }
    }

    diesel::table! {
        pds.account_pref (id) {
            id -> Int4,
            did -> Varchar,
            name -> Varchar,
            valueJson -> Nullable<Text>,
        }
    }

    diesel::table! {
        pds.actor (did) {
            did -> Varchar,
            handle -> Nullable<Varchar>,
            createdAt -> Varchar,
            takedownRef -> Nullable<Varchar>,
            deactivatedAt -> Nullable<Varchar>,
            deleteAfter -> Nullable<Varchar>,
        }
    }

    diesel::table! {
        pds.app_password (did, name) {
            did -> Varchar,
            name -> Varchar,
            password -> Varchar,
            createdAt -> Varchar,
        }
    }

    diesel::table! {
        pds.authorization_request (id) {
            id -> Varchar,
            did -> Nullable<Varchar>,
            device_id -> Nullable<Varchar>,
            client_id -> Varchar,
            cleint_auth -> Varchar,
            parameters -> Varchar,
            expires_at -> Varchar,
            code -> Nullable<Varchar>,
        }
    }

    diesel::table! {
        pds.backlink (uri, path) {
            uri -> Varchar,
            path -> Varchar,
            linkTo -> Varchar,
        }
    }

    diesel::table! {
        pds.blob (cid, did) {
            cid -> Varchar,
            did -> Varchar,
            mimeType -> Varchar,
            size -> Int4,
            tempKey -> Nullable<Varchar>,
            width -> Nullable<Int4>,
            height -> Nullable<Int4>,
            createdAt -> Varchar,
            takedownRef -> Nullable<Varchar>,
        }
    }

    diesel::table! {
        pds.device (id) {
            id -> Varchar,
            session_id -> Nullable<Varchar>,
            user_agent -> Nullable<Varchar>,
            ip_address -> Varchar,
            last_seen_at -> Varchar,
        }
    }

    diesel::table! {
        pds.device_account (device_id, did) {
            did -> Varchar,
            device_id -> Varchar,
            authenticated_at -> Varchar,
            remember -> Bool,
            authorized_clients -> Varchar,
        }
    }

    diesel::table! {
        pds.did_doc (did) {
            did -> Varchar,
            doc -> Text,
            updatedAt -> Int8,
        }
    }

    diesel::table! {
        pds.email_token (purpose, did) {
            purpose -> Varchar,
            did -> Varchar,
            token -> Varchar,
            requestedAt -> Varchar,
        }
    }

    diesel::table! {
        pds.invite_code (code) {
            code -> Varchar,
            availableUses -> Int4,
            disabled -> Int2,
            forAccount -> Varchar,
            createdBy -> Varchar,
            createdAt -> Varchar,
        }
    }

    diesel::table! {
        pds.invite_code_use (code, usedBy) {
            code -> Varchar,
            usedBy -> Varchar,
            usedAt -> Varchar,
        }
    }

    diesel::table! {
        pds.record (uri) {
            uri -> Varchar,
            cid -> Varchar,
            did -> Varchar,
            collection -> Varchar,
            rkey -> Varchar,
            repoRev -> Nullable<Varchar>,
            indexedAt -> Varchar,
            takedownRef -> Nullable<Varchar>,
        }
    }

    diesel::table! {
        pds.record_blob (blobCid, recordUri) {
            blobCid -> Varchar,
            recordUri -> Varchar,
            did -> Varchar,
        }
    }

    diesel::table! {
        pds.refresh_token (id) {
            id -> Varchar,
            did -> Varchar,
            expiresAt -> Varchar,
            nextId -> Nullable<Varchar>,
            appPasswordName -> Nullable<Varchar>,
        }
    }

    diesel::table! {
        pds.repo_block (cid, did) {
            cid -> Varchar,
            did -> Varchar,
            repoRev -> Varchar,
            size -> Int4,
            content -> Bytea,
        }
    }

    diesel::table! {
        pds.repo_root (did) {
            did -> Varchar,
            cid -> Varchar,
            rev -> Varchar,
            indexedAt -> Varchar,
        }
    }

    diesel::table! {
        pds.repo_seq (seq) {
            seq -> Int8,
            did -> Varchar,
            eventType -> Varchar,
            event -> Bytea,
            invalidated -> Int2,
            sequencedAt -> Varchar,
        }
    }

    diesel::table! {
        pds.token (id) {
            id -> Varchar,
            did -> Varchar,
            token_id -> Varchar,
            created_at -> Bool,
            updated_at -> Varchar,
            expires_at -> Varchar,
            client_id -> Varchar,
            client_auth -> Varchar,
            device_id -> Nullable<Varchar>,
            parameters -> Varchar,
            details -> Nullable<Varchar>,
            code -> Nullable<Varchar>,
            current_refresh_token -> Nullable<Varchar>,
        }
    }

    diesel::table! {
        pds.used_refresh_token (refresh_token) {
            refresh_token -> Varchar,
            token_id -> Varchar,
        }
    }

    diesel::allow_tables_to_appear_in_same_query!(
        account,
        account_pref,
        actor,
        app_password,
        authorization_request,
        backlink,
        blob,
        device,
        device_account,
        did_doc,
        email_token,
        invite_code,
        invite_code_use,
        record,
        record_blob,
        refresh_token,
        repo_block,
        repo_root,
        repo_seq,
        token,
        used_refresh_token,
    );
}
