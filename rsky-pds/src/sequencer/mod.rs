use crate::account_manager::helpers::account::AccountStatus;
use crate::crawlers::Crawlers;
use crate::db::establish_connection;
use crate::models;
use crate::repo::types::{CommitData, PreparedWrite};
use crate::sequencer::events::{
    format_seq_account_evt, format_seq_commit, format_seq_handle_update, format_seq_identity_evt,
    format_seq_tombstone,
};
use anyhow::Result;
use diesel::*;

#[derive(Debug)]
pub struct Sequencer {
    pub destroyed: bool,
    pub tries_with_no_results: u64,
    pub crawlers: Crawlers,
    pub last_seen: u64,
}

impl Sequencer {
    pub fn new(crawlers: Crawlers, last_seen: Option<u64>) -> Self {
        Sequencer {
            destroyed: false,
            tries_with_no_results: 0,
            last_seen: last_seen.unwrap_or(0),
            crawlers,
        }
    }

    pub async fn sequence_evt(&mut self, evt: models::RepoSeq) -> Result<()> {
        use crate::schema::pds::repo_seq::dsl as RepoSeqSchema;
        let conn = &mut establish_connection()?;

        insert_into(RepoSeqSchema::repo_seq)
            .values((
                RepoSeqSchema::did.eq(evt.did),
                RepoSeqSchema::event.eq(evt.event),
                RepoSeqSchema::eventType.eq(evt.event_type),
                RepoSeqSchema::sequencedAt.eq(evt.sequenced_at),
            ))
            .execute(conn)?;
        self.crawlers.notify_of_update().await
    }

    pub async fn sequence_commit(
        &mut self,
        did: String,
        commit_data: CommitData,
        writes: Vec<PreparedWrite>,
    ) -> Result<()> {
        let evt = format_seq_commit(did, commit_data, writes).await?;
        self.sequence_evt(evt).await
    }

    pub async fn sequence_handle_update(&mut self, did: String, handle: String) -> Result<()> {
        let evt = format_seq_handle_update(did, handle).await?;
        self.sequence_evt(evt).await
    }

    pub async fn sequence_identity_evt(&mut self, did: String) -> Result<()> {
        let evt = format_seq_identity_evt(did).await?;
        self.sequence_evt(evt).await
    }

    pub async fn sequence_account_evt(&mut self, did: String, status: AccountStatus) -> Result<()> {
        let evt = format_seq_account_evt(did, status).await?;
        self.sequence_evt(evt).await
    }

    pub async fn sequence_tombstone(&mut self, did: String) -> Result<()> {
        let evt = format_seq_tombstone(did).await?;
        self.sequence_evt(evt).await
    }
}

pub async fn delete_all_for_user(did: &String) -> Result<()> {
    use crate::schema::pds::repo_seq::dsl as RepoSeqSchema;
    let conn = &mut establish_connection()?;

    delete(RepoSeqSchema::repo_seq)
        .filter(RepoSeqSchema::did.eq(did))
        .execute(conn)?;
    Ok(())
}

pub mod events;
