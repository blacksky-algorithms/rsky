#![crate_name = "rsky_feedgen"]

//! A Bluesky feed generator that closely follows the use cases of the Blacksky community.
//! It includes an API for receiving records from a firehose subscriber.
//! ## Overview
//!
//! Feed Generators are services that provide custom algorithms to users through the AT Protocol.
//!
//! They work very simply: the server receives a request from a user's server and returns a list 
//! of [post URIs](https://atproto.com/specs/at-uri-scheme) with some optional metadata attached. Those posts are then hydrated into full views by the requesting server and sent back to the client. This route is described in the [`app.bsky.feed.getFeedSkeleton` lexicon](https://atproto.com/lexicons/app-bsky-feed#appbskyfeedgetfeedskeleton).
//!
//! A Feed Generator service can host one or more algorithms. The service itself is identified by DID, while each algorithm that it hosts is declared by a record in the repo of the account that created it. 
//!
//! For instance, feeds offered by Bluesky will likely be declared in `@bsky.app`'s repo. Therefore, a given algorithm is identified by the at-uri of the declaration record. This declaration record includes a pointer to the service's DID along with some profile information for the feed.
//!
//! The general flow of providing a custom algorithm to a user is as follows:
//! - A user requests a feed from their server (PDS) using the at-uri of the declared feed
//! - The PDS resolves the at-uri and finds the DID doc of the Feed Generator
//! - The PDS sends a `getFeedSkeleton` request to the service endpoint declared in the Feed 
//! Generator's DID doc
//!     - This request is authenticated by a JWT signed by the user's repo signing key
//! - The Feed Generator returns a skeleton of the feed to the user's PDS
//! - The PDS hydrates the feed (user info, post contents, aggregates, etc.)
//!     - In the future, the PDS will hydrate the feed with the help of an App View, but for now,
//! the PDS handles hydration itself
//! - The PDS returns the hydrated feed to the user
//!
//! For users, this should feel like visiting a page in the app. Once they subscribe to a custom 
//! algorithm, it will appear in their home interface as one of their available feeds.

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

/// `ReadReplicaConn` is an enum that represents a connection to a read replica database.
/// It provides two variants for different types of read replica connections.
///
/// # Variants
///
/// * `Conn1(ReadReplicaConn1)` - Represents a connection to a read replica of type `ReadReplicaConn1`.
/// * `Conn2(ReadReplicaConn2)` - Represents a connection to a read replica of type `ReadReplicaConn2`.
///
/// This enum enables flexibility in handling different kinds of read replica connections
/// under a single unified type.
pub enum ReadReplicaConn {
    Conn1(ReadReplicaConn1),
    Conn2(ReadReplicaConn2),
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for ReadReplicaConn {
    type Error = ();
    /// An implementation of the `FromRequest` trait for dynamically selecting a read replica connection.
    ///
    /// This function is responsible for creating an instance of `ReadReplicaConn` by randomly deciding
    /// between two possible read replica connections: `ReadReplicaConn1` or `ReadReplicaConn2`.
    ///
    /// # Arguments:
    /// * `req` - A reference to the incoming [`Request`] object provided by the Rocket framework.
    ///
    /// # Returns:
    /// * Returns an [`Outcome`] that represents the result of attempting to generate a `ReadReplicaConn`.
    ///     - `Outcome::Success` contains a `ReadReplicaConn` instance (either `Conn1` or `Conn2`, depending on the random selection).
    ///     - `Outcome::Error` contains an error encountered during the attempt to connect to the chosen replica.
    ///     - `Outcome::Forward` forwards the request when no successful connection could be established.
    ///
    /// # Behavior:
    /// - A random number generator (`rand::thread_rng()`) is used to produce either `0` or `1`.
    ///   - Based on the result:
    ///     - If the random number is `0`, the function attempts to instantiate `ReadReplicaConn1` by calling its own
    ///       `from_request` function.
    ///     - If the random number is `1`, the function attempts to instantiate `ReadReplicaConn2` in the same way.
    /// - A log message (`@LOG`) is printed to indicate which read replica (`read_replica_0` or `read_replica_1`) is being used.
    /// - The `Outcome` object of the invoked replica's `from_request` is then mapped to wrap the selected connection type
    ///   in a `ReadReplicaConn` enum variant (`Conn1` or `Conn2`).
    ///
    /// # Dependency:
    /// This function relies on `rand` crate for generating random numbers. It is assumed that
    /// `ReadReplicaConn1` and `ReadReplicaConn2` implement the `FromRequest` trait themselves.
    ///
    /// # Example:
    /// ```rust
    /// async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
    ///     // The logic described here will pick one of the read replica connections randomly.
    /// }
    /// ```
    ///
    /// # Errors:
    /// - If `ReadReplicaConn1` or `ReadReplicaConn2` fails to instantiate, the function returns an `Outcome::Error`.
    /// - If the request is supposed to be forwarded without handling, an `Outcome::Forward` is returned.
    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let n: u8 = {
            let mut rng = rand::thread_rng();
            // generates either 0 or 1
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
    /// Executes an asynchronous operation on a database connection, selecting the appropriate
    /// connection from a set of read replica connections.
    ///
    /// # Parameters
    ///
    /// * `f`: A closure that takes a mutable reference to a `PgConnection` and
    ///        performs an operation, returning a result of type `R`.
    ///
    /// # Type Constraints
    ///
    /// * `F`: Must be a closure or function that implements `FnOnce(&mut PgConnection) -> R`.
    ///        It must also be `Send` and have a static lifetime (`'static`).
    /// * `R`: The return type of the closure, which must implement `Send` and
    ///        have a static lifetime (`'static`).
    ///
    /// # Returns
    ///
    /// Returns the result of type `R` produced by the execution of the provided closure on
    /// the selected database connection.
    ///
    /// # Behavior
    ///
    /// Based on the variant of `ReadReplicaConn`:
    ///
    /// * If the `ReadReplicaConn` is `Conn1`, it executes the function `f` on `conn1` and awaits the result.
    /// * If the `ReadReplicaConn` is `Conn2`, it executes the function `f` on `conn2` and awaits the result.
    ///
    /// This design allows for load balancing or read-scaling across multiple read-replica database
    /// connections while using a unified interface.
    ///
    /// # Examples
    ///
    /// ```rust
    /// let result = read_replica_conn.run(|conn| {
    ///     // Perform some database operation on `conn`
    ///     query_function(conn)
    /// }).await;
    /// ```
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

/// Configuration for the feed generator.
///
/// This struct allows you to customize the behavior of a feed. You are able
/// to configure:
///- sponsored posts should be shown in a feed
///- the sponsored post's link (uri)
///- the likelihood a sponsored post will be shown to the user
///- the minimum threshold a post needs to meet for it to be considered trending.
///
/// # Example
///```rust
/// use rsky_feedgen::FeedGenConfig;
/// let config = FeedGenConfig {
///    show_sponsored_post: false,
///    sponsored_post_uri: "at://did:example/sponsored-post".to_string(),
///    sponsored_post_probability: 0.3,
///    trending_percentile_min: 0.9,
///};
/// ```
#[derive(Clone)]
pub struct FeedGenConfig {
    /// determines whether a sponsored post will be shown in a feed
    pub show_sponsored_post: bool,
    /// displays the link (uri) of the sponsored post
    pub sponsored_post_uri: String,
    /// determines how likely a sponsored post will be shown in the feed
    pub sponsored_post_probability: f64,
    /// determines the minimum threshold a post needs to meet for it to be considered trending.
    pub trending_percentile_min: f64,
}

pub mod apis;
pub mod auth;
pub mod db;
pub mod models;
pub mod routes;
pub mod schema;
