use libipld::Cid;
use helpers::pcrypt;

/// Helps with readability when calling create_account()
pub struct CreateAccountOpts {
    pub did: String,
    pub handle: String,
    pub email: Option<String>,
    pub password: Option<String>,
    pub repo_cid: Cid,
    pub repo_rev: String,
    pub invite_code: Option<String>,
    pub deactivated: Option<bool>
}

pub struct AccountManager {}

impl AccountManager {
    pub fn create_account(opts: CreateAccountOpts) -> Result<()> {
        let CreateAccountOpts {
            did,
            handle,
            email,
            password,
            repo_cid,
            repo_rev,
            invite_code,
            deactivated
        } = opts;
        let password_encrypted: Option<String> = if let Some(password) = password {
            Some(pcrypt::gen_salt_and_hash(password)?)
        } else {
            None
        };
        
        Ok(())
    }
}

pub mod helpers;