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
            deviceId -> Nullable<Varchar>,
            clientId -> Varchar,
            clientAuth -> Varchar,
            parameters -> Varchar,
            expiresAt -> Timestamptz,
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
            sessionId -> Nullable<Varchar>,
            userAgent -> Nullable<Varchar>,
            ipAddress -> Varchar,
            lastSeenAt -> Timestamptz,
        }
    }

    diesel::table! {
        pds.device_account (deviceId, did) {
            did -> Varchar,
            deviceId -> Varchar,
            authenticatedAt -> Timestamptz,
            remember -> Bool,
            authorizedClients -> Varchar,
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
            tokenId -> Varchar,
            createdAt -> Timestamptz,
            updatedAt -> Timestamptz,
            expiresAt -> Timestamptz,
            clientId -> Varchar,
            clientAuth -> Varchar,
            deviceId -> Nullable<Varchar>,
            parameters -> Varchar,
            details -> Nullable<Varchar>,
            code -> Nullable<Varchar>,
            currentRefreshToken -> Nullable<Varchar>,
        }
    }

    diesel::table! {
        pds.used_refresh_token (refreshToken) {
            refreshToken -> Varchar,
            tokenId -> Varchar,
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
