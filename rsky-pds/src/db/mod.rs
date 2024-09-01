use anyhow::Result;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use dotenvy::dotenv;
use std::env;

pub fn establish_connection() -> Result<PgConnection> {
    dotenv().ok();

    let database_url = env::var("DATABASE_URL").unwrap_or("".into());
    let result = PgConnection::establish(&database_url).map_err(|error| {
        let context = format!("Error connecting to {database_url:?}");
        anyhow::Error::new(error).context(context)
    })?;

    Ok(result)
}
