use crate::models::Post;
use surrealdb::engine::local::Db;
use surrealdb::{Error as SurrealError, Surreal};

pub async fn save_post(db: &Surreal<Db>, post: Post) -> Result<(), SurrealError> {
    // Use the post.uri as the record ID in SurrealDB (post:uri)
    db.update::<Option<Post>>(("post", &post.uri))
        .content(post)
        .await?;
    Ok(())
}
