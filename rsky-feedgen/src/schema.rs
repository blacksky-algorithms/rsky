// @generated automatically by Diesel CLI.

diesel::table! {
    kysely_migration (name) {
        #[max_length = 255]
        name -> Varchar,
        #[max_length = 255]
        timestamp -> Varchar,
    }
}

diesel::table! {
    kysely_migration_lock (id) {
        #[max_length = 255]
        id -> Varchar,
        is_locked -> Int4,
    }
}

diesel::table! {
    like (uri) {
        uri -> Varchar,
        cid -> Varchar,
        author -> Varchar,
        subjectCid -> Varchar,
        subjectUri -> Varchar,
        createdAt -> Varchar,
        indexedAt -> Varchar,
        prev -> Nullable<Varchar>,
        sequence -> Nullable<Int8>,
    }
}

diesel::table! {
    membership (did) {
        did -> Varchar,
        included -> Bool,
        excluded -> Bool,
        list -> Varchar,
    }
}

diesel::table! {
    post (uri) {
        uri -> Varchar,
        cid -> Varchar,
        replyParent -> Nullable<Varchar>,
        replyRoot -> Nullable<Varchar>,
        indexedAt -> Varchar,
        prev -> Nullable<Varchar>,
        sequence -> Nullable<Int8>,
    }
}

diesel::table! {
    sub_state (service) {
        service -> Varchar,
        cursor -> Int8,
    }
}

diesel::table! {
    visitor (id) {
        id -> Int4,
        did -> Varchar,
        web -> Varchar,
        visited_at -> Varchar,
    }
}

diesel::allow_tables_to_appear_in_same_query!(
    kysely_migration,
    kysely_migration_lock,
    like,
    membership,
    post,
    sub_state,
    visitor,
);
