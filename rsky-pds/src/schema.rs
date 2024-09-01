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

    diesel::allow_tables_to_appear_in_same_query!(
        account,
        account_pref,
        actor,
        app_password,
        backlink,
        blob,
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
    );
}
