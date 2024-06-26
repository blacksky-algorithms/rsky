use crate::auth_verifier::AuthScope;
use crate::db::establish_connection;
use crate::models;
use crate::repo::preference::util::pref_in_scope;
use anyhow::{bail, Result};
use diesel::*;
use rsky_lexicon::app::bsky::actor::RefPreferences;

pub struct PreferenceReader {
    pub did: String,
}

impl PreferenceReader {
    pub fn new(did: String) -> Self {
        PreferenceReader { did }
    }

    pub async fn get_preferences(
        &self,
        namespace: Option<String>,
        scope: AuthScope,
    ) -> Result<Vec<RefPreferences>> {
        use crate::schema::pds::account_pref::dsl as AccountPrefSchema;
        let conn = &mut establish_connection()?;

        let prefs_res = AccountPrefSchema::account_pref
            .filter(AccountPrefSchema::did.eq(&self.did))
            .select(models::AccountPref::as_select())
            .order(AccountPrefSchema::id.asc())
            .load(conn)?;
        let account_prefs = prefs_res
            .into_iter()
            .filter(|pref| match &namespace {
                None => true,
                Some(namespace) => pref_match_namespace(&namespace, &pref.name),
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
    }
}

pub fn pref_match_namespace(namespace: &String, fullname: &String) -> bool {
    fullname == namespace || fullname.starts_with(&format!("{namespace}."))
}

pub mod util;
