use crate::actor_store::preference::util::pref_in_scope;
use crate::auth_verifier::AuthScope;
use crate::db::DbConn;
use crate::models;
use crate::models::AccountPref;
use anyhow::{bail, Result};
use diesel::*;
use rsky_lexicon::app::bsky::actor::RefPreferences;
use std::sync::Arc;

pub struct PreferenceReader {
    pub did: String,
    pub db: Arc<DbConn>,
}

impl PreferenceReader {
    pub fn new(did: String, db: Arc<DbConn>) -> Self {
        PreferenceReader { did, db }
    }

    pub async fn get_preferences(
        &self,
        namespace: Option<String>,
        scope: AuthScope,
    ) -> Result<Vec<RefPreferences>> {
        use crate::schema::pds::account_pref::dsl as AccountPrefSchema;

        let did = self.did.clone();
        self.db
            .run(move |conn| {
                let prefs_res = AccountPrefSchema::account_pref
                    .filter(AccountPrefSchema::did.eq(&did))
                    .select(AccountPref::as_select())
                    .order(AccountPrefSchema::id.asc())
                    .load(conn)?;
                let account_prefs = prefs_res
                    .into_iter()
                    .filter(|pref| match &namespace {
                        None => true,
                        Some(namespace) => pref_match_namespace(namespace, &pref.name),
                    })
                    .filter(|pref| pref_in_scope(scope.clone(), pref.name.clone()))
                    .map(|pref| {
                        let value_json_res = match pref.value_json {
                            None => bail!("preferences json null for {}", pref.name),
                            Some(value_json) => serde_json::from_str::<RefPreferences>(&value_json),
                        };
                        match value_json_res {
                            Err(error) => bail!(error.to_string()),
                            Ok(value_json) => Ok(value_json),
                        }
                    })
                    .collect::<Result<Vec<RefPreferences>>>()?;
                Ok(account_prefs)
            })
            .await
    }

    #[tracing::instrument(skip_all)]
    pub async fn put_preferences(
        &self,
        values: Vec<RefPreferences>,
        namespace: String,
        scope: AuthScope,
    ) -> Result<()> {
        let did = self.did.clone();
        self.db
            .run(move |conn| {
                match values
                    .iter()
                    .all(|value| pref_match_namespace(&namespace, &value.get_type()))
                {
                    false => bail!("Some preferences are not in the {namespace} namespace"),
                    true => {
                        let not_in_scope = values
                            .iter()
                            .filter(|value| !pref_in_scope(scope.clone(), value.get_type()))
                            .collect::<Vec<&RefPreferences>>();
                        if !not_in_scope.is_empty() {
                            tracing::info!(
                        "@LOG: PreferenceReader::put_preferences() debug scope: {:?}, values: {:?}",
                        scope,
                        values
                    );
                            bail!("Do not have authorization to set preferences.");
                        }
                        // get all current prefs for user and prep new pref rows
                        use crate::schema::pds::account_pref::dsl as AccountPrefSchema;
                        let all_prefs = AccountPrefSchema::account_pref
                            .filter(AccountPrefSchema::did.eq(&did))
                            .select(models::AccountPref::as_select())
                            .load(conn)?;
                        let put_prefs = values
                            .into_iter()
                            .map(|value| {
                                Ok(AccountPref {
                                    id: 0,
                                    name: value.get_type(),
                                    value_json: Some(serde_json::to_string(&value)?),
                                })
                            })
                            .collect::<Result<Vec<AccountPref>>>()?;

                        let all_pref_ids_in_namespace = all_prefs
                            .iter()
                            .filter(|pref| pref_match_namespace(&namespace, &pref.name))
                            .filter(|pref| pref_in_scope(scope.clone(), pref.name.clone()))
                            .map(|pref| pref.id)
                            .collect::<Vec<i32>>();
                        // replace all prefs in given namespace
                        if !all_pref_ids_in_namespace.is_empty() {
                            delete(AccountPrefSchema::account_pref)
                                .filter(AccountPrefSchema::id.eq_any(all_pref_ids_in_namespace))
                                .execute(conn)?;
                        }
                        if !put_prefs.is_empty() {
                            insert_into(AccountPrefSchema::account_pref)
                                .values(
                                    put_prefs
                                        .into_iter()
                                        .map(|pref| {
                                            (
                                                AccountPrefSchema::did.eq(&did),
                                                AccountPrefSchema::name.eq(pref.name),
                                                AccountPrefSchema::valueJson.eq(pref.value_json),
                                            )
                                        })
                                        .collect::<Vec<_>>(),
                                )
                                .execute(conn)?;
                        }
                        Ok(())
                    }
                }
            })
            .await
    }
}

pub fn pref_match_namespace(namespace: &String, fullname: &String) -> bool {
    fullname == namespace || fullname.starts_with(&format!("{namespace}."))
}

pub mod util;
