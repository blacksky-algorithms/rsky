// @generated automatically by Diesel CLI.

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
    membership,
    post,
    sub_state,
    visitor,
);
