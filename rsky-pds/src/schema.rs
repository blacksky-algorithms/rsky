// @generated automatically by Diesel CLI.

diesel::table! {
    app_migration (id) {
        id -> Varchar,
        success -> Int2,
        completedAt -> Nullable<Varchar>,
    }
}

diesel::table! {
    app_password (did, name) {
        did -> Varchar,
        name -> Varchar,
        passwordScrypt -> Varchar,
        createdAt -> Varchar,
    }
}

diesel::table! {
    backlink (uri, path) {
        uri -> Varchar,
        path -> Varchar,
        linkToUri -> Nullable<Varchar>,
        linkToDid -> Nullable<Varchar>,
    }
}

diesel::table! {
    follow (uri) {
        uri -> Varchar,
        cid -> Varchar,
        author -> Varchar,
        subject -> Varchar,
        createdAt -> Varchar,
        indexedAt -> Varchar,
        prev -> Nullable<Varchar>,
        sequence -> Nullable<Int8>,
    }
}

diesel::table! {
    image (cid) {
        cid -> Varchar,
        alt -> Nullable<Varchar>,
        postCid -> Varchar,
        postUri -> Varchar,
        createdAt -> Varchar,
        indexedAt -> Varchar,
        labels -> Nullable<Array<Nullable<Text>>>,
    }
}

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
    membership (did, list) {
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
        text -> Nullable<Varchar>,
        lang -> Nullable<Varchar>,
        author -> Varchar,
        externalUri -> Nullable<Varchar>,
        externalTitle -> Nullable<Varchar>,
        externalDescription -> Nullable<Varchar>,
        externalThumb -> Nullable<Varchar>,
        quoteCid -> Nullable<Varchar>,
        quoteUri -> Nullable<Varchar>,
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
        feed -> Nullable<Varchar>,
    }
}

diesel::allow_tables_to_appear_in_same_query!(
    app_migration,
    app_password,
    backlink,
    follow,
    image,
    kysely_migration,
    kysely_migration_lock,
    like,
    membership,
    post,
    sub_state,
    visitor,
);
