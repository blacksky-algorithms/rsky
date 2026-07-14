use crate::actor_store::db::ActorDb;
use crate::actor_store::preference::util::pref_in_scope;
use crate::auth_verifier::AuthScope;
use anyhow::{bail, Result};
use rsky_lexicon::app::bsky::actor::RefPreferences;

pub struct PreferenceReader {
    pub did: String,
    pub db: ActorDb,
}

impl PreferenceReader {
    pub fn new(did: String, db: ActorDb) -> Self {
        PreferenceReader { did, db }
    }

    pub async fn get_preferences(
        &self,
        namespace: Option<String>,
        scope: AuthScope,
    ) -> Result<Vec<RefPreferences>> {
        let rows: Vec<(String, String)> = self
            .db
            .run(|conn| {
                let mut stmt =
                    conn.prepare("SELECT name, \"valueJson\" FROM account_pref ORDER BY id ASC")?;
                let rows = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect::<Result<Vec<(String, String)>, rusqlite::Error>>()?;
                Ok(rows)
            })
            .await?;
        rows.into_iter()
            .filter(|(name, _)| match &namespace {
                None => true,
                Some(namespace) => pref_match_namespace(namespace, name),
            })
            .filter(|(name, _)| pref_in_scope(scope.clone(), name.clone()))
            .map(
                |(name, value_json)| match serde_json::from_str::<RefPreferences>(&value_json) {
                    Ok(pref) => Ok(pref),
                    Err(error) => bail!("preferences json invalid for {name}: {error}"),
                },
            )
            .collect::<Result<Vec<RefPreferences>>>()
    }

    #[tracing::instrument(skip_all)]
    pub async fn put_preferences(
        &self,
        values: Vec<RefPreferences>,
        namespace: String,
        scope: AuthScope,
    ) -> Result<()> {
        if !values
            .iter()
            .all(|value| pref_match_namespace(&namespace, &value.get_type()))
        {
            bail!("Some preferences are not in the {namespace} namespace")
        }
        let not_in_scope = values
            .iter()
            .filter(|value| !pref_in_scope(scope.clone(), value.get_type()))
            .collect::<Vec<&RefPreferences>>();
        if !not_in_scope.is_empty() {
            bail!("Do not have authorization to set preferences.");
        }
        let put_prefs = values
            .into_iter()
            .map(|value| Ok((value.get_type(), serde_json::to_string(&value)?)))
            .collect::<Result<Vec<(String, String)>>>()?;
        self.db
            .tx(move |tx| {
                // get all current prefs for user and replace all prefs in given namespace
                let mut stmt = tx.prepare("SELECT id, name FROM account_pref")?;
                let all_prefs = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect::<Result<Vec<(i64, String)>, rusqlite::Error>>()?;
                drop(stmt);
                let all_pref_ids_in_namespace = all_prefs
                    .iter()
                    .filter(|(_, name)| pref_match_namespace(&namespace, name))
                    .filter(|(_, name)| pref_in_scope(scope.clone(), name.clone()))
                    .map(|(id, _)| *id)
                    .collect::<Vec<i64>>();
                if !all_pref_ids_in_namespace.is_empty() {
                    let sql = format!(
                        "DELETE FROM account_pref WHERE id IN ({})",
                        crate::actor_store::repo::sql_repo::placeholders(
                            all_pref_ids_in_namespace.len()
                        )
                    );
                    tx.execute(
                        &sql,
                        rusqlite::params_from_iter(all_pref_ids_in_namespace.iter()),
                    )?;
                }
                if !put_prefs.is_empty() {
                    let mut stmt = tx.prepare(
                        "INSERT INTO account_pref (name, \"valueJson\") VALUES (?1, ?2)",
                    )?;
                    for (name, value_json) in &put_prefs {
                        stmt.execute(rusqlite::params![name, value_json])?;
                    }
                }
                Ok(())
            })
            .await
    }
}

pub fn pref_match_namespace(namespace: &str, fullname: &str) -> bool {
    fullname == namespace || fullname.starts_with(&format!("{namespace}."))
}

pub mod util;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor_store::db::get_migrated_db;

    async fn test_reader() -> (tempfile::TempDir, PreferenceReader) {
        let dir = tempfile::tempdir().unwrap();
        let db = get_migrated_db(dir.path().join("store.sqlite"))
            .await
            .unwrap();
        let reader = PreferenceReader::new("did:example:alice".to_owned(), db);
        (dir, reader)
    }

    fn pref(value: serde_json::Value) -> RefPreferences {
        serde_json::from_value(value).unwrap()
    }

    fn adult_content_pref(enabled: bool) -> RefPreferences {
        pref(serde_json::json!({
            "$type": "app.bsky.actor.defs#adultContentPref",
            "enabled": enabled,
        }))
    }

    fn personal_details_pref() -> RefPreferences {
        pref(serde_json::json!({
            "$type": "app.bsky.actor.defs#personalDetailsPref",
            "birthDate": "2000-01-01T00:00:00.000Z",
        }))
    }

    #[tokio::test]
    async fn puts_and_gets_preferences() {
        let (_dir, reader) = test_reader().await;
        assert!(reader
            .get_preferences(None, AuthScope::Access)
            .await
            .unwrap()
            .is_empty());

        reader
            .put_preferences(
                vec![adult_content_pref(true), personal_details_pref()],
                "app.bsky".to_owned(),
                AuthScope::Access,
            )
            .await
            .unwrap();
        let prefs = reader
            .get_preferences(Some("app.bsky".to_owned()), AuthScope::Access)
            .await
            .unwrap();
        assert_eq!(prefs.len(), 2);

        // namespace filtering
        assert!(reader
            .get_preferences(Some("com.example".to_owned()), AuthScope::Access)
            .await
            .unwrap()
            .is_empty());

        // full-access-only prefs are hidden from app password scope
        let app_pass_prefs = reader
            .get_preferences(Some("app.bsky".to_owned()), AuthScope::AppPass)
            .await
            .unwrap();
        assert_eq!(app_pass_prefs.len(), 1);

        // replaces prefs within the namespace
        reader
            .put_preferences(
                vec![adult_content_pref(false)],
                "app.bsky".to_owned(),
                AuthScope::Access,
            )
            .await
            .unwrap();
        let prefs = reader
            .get_preferences(Some("app.bsky".to_owned()), AuthScope::Access)
            .await
            .unwrap();
        assert_eq!(prefs.len(), 1);
    }

    #[tokio::test]
    async fn app_pass_scope_preserves_full_access_prefs() {
        let (_dir, reader) = test_reader().await;
        reader
            .put_preferences(
                vec![adult_content_pref(true), personal_details_pref()],
                "app.bsky".to_owned(),
                AuthScope::Access,
            )
            .await
            .unwrap();
        // app-password writes cannot delete the personal details pref
        reader
            .put_preferences(
                vec![adult_content_pref(false)],
                "app.bsky".to_owned(),
                AuthScope::AppPass,
            )
            .await
            .unwrap();
        let prefs = reader
            .get_preferences(Some("app.bsky".to_owned()), AuthScope::Access)
            .await
            .unwrap();
        assert_eq!(prefs.len(), 2);
    }

    #[tokio::test]
    async fn rejects_invalid_puts() {
        let (_dir, reader) = test_reader().await;
        // outside the namespace
        let res = reader
            .put_preferences(
                vec![adult_content_pref(true)],
                "com.example".to_owned(),
                AuthScope::Access,
            )
            .await;
        assert!(res.is_err());
        // not in scope
        let res = reader
            .put_preferences(
                vec![personal_details_pref()],
                "app.bsky".to_owned(),
                AuthScope::AppPass,
            )
            .await;
        assert!(res.is_err());
        assert!(reader
            .get_preferences(None, AuthScope::Access)
            .await
            .unwrap()
            .is_empty());
    }

    #[test]
    fn unknown_pref_type_in_namespace_is_accepted() {
        let pref: RefPreferences = serde_json::from_value(serde_json::json!({
            "$type": "app.bsky.actor.defs#skyfeedBuilderFeedsPref",
            "feeds": [],
        }))
        .unwrap();
        let namespace = "app.bsky".to_string();
        assert!(pref_match_namespace(&namespace, &pref.get_type()));
        assert!(util::pref_in_scope(AuthScope::AppPass, pref.get_type()));
    }

    #[test]
    fn pref_outside_namespace_is_rejected() {
        let pref: RefPreferences = serde_json::from_value(serde_json::json!({
            "$type": "com.example.defs#somePref",
        }))
        .unwrap();
        assert!(!pref_match_namespace("app.bsky", &pref.get_type()));
    }

    #[test]
    fn pref_without_type_is_rejected() {
        let pref: RefPreferences =
            serde_json::from_value(serde_json::json!({ "enabled": true })).unwrap();
        assert!(!pref_match_namespace("app.bsky", &pref.get_type()));
    }
}
