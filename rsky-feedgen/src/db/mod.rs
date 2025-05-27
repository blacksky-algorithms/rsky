use diesel::pg::PgConnection;
use diesel::prelude::*;
use dotenvy::dotenv;
use std::env;

/// Establishes a connection to the PostgreSQL database using the Diesel library.
///
/// This function reads the database connection URL from the `DATABASE_URL`
/// environment variable. If the variable is not set or the connection fails, it
/// will return an error. The `.env` file is loaded automatically using the `dotenv`
/// crate to populate environment variables.
///
/// # Returns
///
/// * `Ok(PgConnection)` - A successful database connection.
/// * `Err(Box<dyn std::error::Error>)` - Returns an error if the environment variable
///   is missing or if the connection to the database could not be established.
///
/// # Errors
///
/// 1. If the `DATABASE_URL` environment variable is not set, the function will
///    attempt to connect with an empty string, which results in an error.
/// 2. If the connection to the database fails, an error is returned and a message
///    is printed to the standard error output.
///
/// # Example
///
/// ```rust
/// use rsky_feedgen::db::establish_connection;
///
/// fn main() {
///     let connection = &mut establish_connection()?;
/// }
/// ```
pub fn establish_connection() -> Result<PgConnection, Box<dyn std::error::Error>> {
    dotenv().ok();

    let database_url = env::var("DATABASE_URL").unwrap_or("".into());
    let result = PgConnection::establish(&database_url).map_err(|_| {
        eprintln!("Error connecting to {database_url:?}");
        "Internal error"
    })?;

    Ok(result)
}
