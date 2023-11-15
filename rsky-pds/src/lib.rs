use diesel::pg::PgConnection;
use rocket_sync_db_pools::database;

#[database("pg_db")]
pub struct DbConn(PgConnection);