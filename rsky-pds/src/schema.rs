// @generated automatically by Diesel CLI.

pub mod pds {
    diesel::table! {
        pds.app_migration (id) {
            id -> Varchar,
            success -> Int2,
            completedAt -> Nullable<Varchar>,
        }
    }

    diesel::table! {
        pds.app_password (did, name) {
            did -> Varchar,
            name -> Varchar,
            passwordScrypt -> Varchar,
            createdAt -> Varchar,
        }
    }

    diesel::table! {
        pds.backlink (uri, path) {
            uri -> Varchar,
            path -> Varchar,
            linkToUri -> Nullable<Varchar>,
            linkToDid -> Nullable<Varchar>,
        }
    }

    diesel::table! {
        pds.blob (creator, cid) {
            creator -> Varchar,
            cid -> Varchar,
            mimeType -> Varchar,
            size -> Int4,
            tempKey -> Nullable<Varchar>,
            width -> Nullable<Int4>,
            height -> Nullable<Int4>,
            createdAt -> Varchar,
        }
    }

    diesel::table! {
        pds.did_cache (did) {
            did -> Varchar,
            doc -> Text,
            updatedAt -> Int8,
        }
    }

    diesel::table! {
        pds.did_handle (did) {
            did -> Varchar,
            handle -> Nullable<Varchar>,
        }
    }

    diesel::table! {
        pds.email_token (purpose, did) {
            purpose -> Varchar,
            did -> Varchar,
            token -> Varchar,
            requestedAt -> Timestamptz,
        }
    }

    diesel::table! {
        pds.invite_code (code) {
            code -> Varchar,
            availableUses -> Int4,
            disabled -> Int2,
            forUser -> Varchar,
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
        pds.ipld_block (creator, cid) {
            creator -> Varchar,
            cid -> Varchar,
            size -> Int4,
            content -> Bytea,
            repoRev -> Nullable<Varchar>,
        }
    }

    diesel::table! {
        pds.moderation_action (id) {
            id -> Int4,
            action -> Varchar,
            subjectType -> Varchar,
            subjectDid -> Varchar,
            subjectUri -> Nullable<Varchar>,
            subjectCid -> Nullable<Varchar>,
            reason -> Text,
            createdAt -> Varchar,
            createdBy -> Varchar,
            reversedAt -> Nullable<Varchar>,
            reversedBy -> Nullable<Varchar>,
            reversedReason -> Nullable<Text>,
            createLabelVals -> Nullable<Varchar>,
            negateLabelVals -> Nullable<Varchar>,
            durationInHours -> Nullable<Int4>,
            expiresAt -> Nullable<Varchar>,
        }
    }

    diesel::table! {
        pds.moderation_action_subject_blob (actionId, cid, recordUri) {
            id -> Int4,
            actionId -> Int4,
            cid -> Varchar,
            recordUri -> Varchar,
        }
    }

    diesel::table! {
        pds.moderation_report (id) {
            id -> Int4,
            subjectType -> Varchar,
            subjectDid -> Varchar,
            subjectUri -> Nullable<Varchar>,
            subjectCid -> Nullable<Varchar>,
            reasonType -> Varchar,
            reason -> Nullable<Text>,
            reportedByDid -> Varchar,
            createdAt -> Varchar,
        }
    }

    diesel::table! {
        pds.moderation_report_resolution (reportId, actionId) {
            reportId -> Int4,
            actionId -> Int4,
            createdAt -> Varchar,
            createdBy -> Varchar,
        }
    }

    diesel::table! {
        pds.record (uri) {
            uri -> Varchar,
            cid -> Varchar,
            did -> Varchar,
            collection -> Varchar,
            rkey -> Varchar,
            indexedAt -> Varchar,
            takedownRef -> Nullable<Varchar>,
            repoRev -> Nullable<Varchar>,
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
        pds.repo_blob (cid, recordUri) {
            cid -> Varchar,
            recordUri -> Varchar,
            did -> Varchar,
            takedownRef -> Nullable<Varchar>,
            repoRev -> Nullable<Varchar>,
        }
    }

    diesel::table! {
        pds.repo_root (did) {
            did -> Varchar,
            root -> Varchar,
            indexedAt -> Varchar,
            takedownRef -> Nullable<Varchar>,
            rev -> Nullable<Varchar>,
        }
    }

    diesel::table! {
        pds.repo_seq (id) {
            id -> Int8,
            seq -> Nullable<Int8>,
            did -> Varchar,
            eventType -> Varchar,
            event -> Bytea,
            invalidated -> Int2,
            sequencedAt -> Varchar,
        }
    }

    diesel::table! {
        pds.runtime_flag (name) {
            name -> Varchar,
            value -> Varchar,
        }
    }

    diesel::table! {
        pds.user_account (did) {
            did -> Varchar,
            email -> Varchar,
            passwordScrypt -> Varchar,
            createdAt -> Varchar,
            invitesDisabled -> Int2,
            inviteNote -> Nullable<Varchar>,
            emailConfirmedAt -> Nullable<Varchar>,
        }
    }

    diesel::table! {
        pds.user_pref (id) {
            id -> Int8,
            did -> Varchar,
            name -> Varchar,
            valueJson -> Text,
        }
    }

    diesel::joinable!(moderation_action_subject_blob -> moderation_action (actionId));
    diesel::joinable!(moderation_report_resolution -> moderation_action (actionId));
    diesel::joinable!(moderation_report_resolution -> moderation_report (reportId));

    diesel::allow_tables_to_appear_in_same_query!(
        app_migration,
        app_password,
        backlink,
        blob,
        did_cache,
        did_handle,
        email_token,
        invite_code,
        invite_code_use,
        ipld_block,
        moderation_action,
        moderation_action_subject_blob,
        moderation_report,
        moderation_report_resolution,
        record,
        refresh_token,
        repo_blob,
        repo_root,
        repo_seq,
        runtime_flag,
        user_account,
        user_pref,
    );
}
