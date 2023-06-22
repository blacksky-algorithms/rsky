// @generated automatically by Diesel CLI.

diesel::table! {
    post (uri) {
        uri -> Varchar,
        cid -> Varchar,
        replyParent -> Nullable<Varchar>,
        replyRoot -> Nullable<Varchar>,
        indexedAt -> Varchar,
    }
}

diesel::table! {
    sub_state (service) {
        service -> Varchar,
        cursor -> Int4,
    }
}

diesel::allow_tables_to_appear_in_same_query!(
    post,
    sub_state,
);
