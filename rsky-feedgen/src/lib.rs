#[macro_use]
extern crate serde_derive;

extern crate serde;
extern crate serde_json;

extern crate rsky_lexicon;

use diesel::pg::PgConnection;
use rocket::request::{FromRequest, Outcome};
use rocket::Request;
use rocket_sync_db_pools::database;

#[database("pg_db")]
pub struct WriteDbConn(PgConnection);

#[database("pg_read_replica_1")]
pub struct ReadReplicaConn1(PgConnection);

#[database("pg_read_replica_2")]
pub struct ReadReplicaConn2(PgConnection);

use rand::Rng;

pub enum ReadReplicaConn {
    Conn1(ReadReplicaConn1),
    Conn2(ReadReplicaConn2),
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for ReadReplicaConn {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let n: u8 = {
            let mut rng = rand::thread_rng();
            rng.gen_range(0..2)
        };
        println!("@LOG: using read_replica_{n}");
        if n == 0 {
            let conn1 = ReadReplicaConn1::from_request(req).await;
            match conn1 {
                Outcome::Success(conn1) => Outcome::Success(ReadReplicaConn::Conn1(conn1)),
                Outcome::Error(e) => Outcome::Error(e),
                Outcome::Forward(status) => Outcome::Forward(status),
            }
        } else {
            let conn2 = ReadReplicaConn2::from_request(req).await;
            match conn2 {
                Outcome::Success(conn2) => Outcome::Success(ReadReplicaConn::Conn2(conn2)),
                Outcome::Error(e) => Outcome::Error(e),
                Outcome::Forward(status) => Outcome::Forward(status),
            }
        }
    }
}

impl ReadReplicaConn {
    pub async fn run<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut PgConnection) -> R + Send + 'static,
        R: Send + 'static,
    {
        match self {
            ReadReplicaConn::Conn1(conn1) => conn1.run(f).await,
            ReadReplicaConn::Conn2(conn2) => conn2.run(f).await,
        }
    }
}

#[derive(Clone)]
pub struct FeedGenConfig {
    pub show_sponsored_post: bool,
    pub sponsored_post_uri: String,
    pub sponsored_post_probability: f64,
    pub trending_percentile_min: f64,
}

pub mod apis;
pub mod auth;
pub mod db;
pub mod explicit_slurs;
pub mod models;
pub mod routes;
pub mod schema;
