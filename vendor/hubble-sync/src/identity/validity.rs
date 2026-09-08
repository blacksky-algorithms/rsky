use std::time::Duration;

use crate::{Did, DidMethod};

/// no refresh allowed for this period
const REFRESH_MINIMUM_AGE: Duration = Duration::from_secs(60);

/// no refresh triggered for this period.
const DID_PLC_VALID_FOR: Duration = Duration::from_secs(21_600);

/// stale values considered still-usable for this period.
const DID_PLC_EXPIRE_AT: Duration = Duration::from_secs(3 * 86_400);

/// no refresh triggered for this period. did:webs never hard-expire.
const DID_WEB_VALID_FOR: Duration = Duration::from_secs(86_400);

pub enum Validity {
    Valid,
    Stale,
    Expired,
}

impl Validity {
    pub fn for_identity(did: &Did, age: Duration) -> Validity {
        match did.method() {
            DidMethod::Plc if age < DID_PLC_VALID_FOR => Self::Valid,
            DidMethod::Plc if age < DID_PLC_EXPIRE_AT => Self::Stale,
            DidMethod::Plc => Self::Expired,
            DidMethod::Web if age < DID_WEB_VALID_FOR => Self::Valid,
            DidMethod::Web => Self::Stale,
        }
    }

    pub fn can_refresh(_did: &Did, age: Duration) -> bool {
        age >= REFRESH_MINIMUM_AGE
    }

    pub fn duration_until_refresh(age: Duration) -> Duration {
        REFRESH_MINIMUM_AGE.saturating_sub(age)
    }

    pub fn should_refresh(&self) -> bool {
        match self {
            Self::Valid => false,
            Self::Stale | Self::Expired => true,
        }
    }

    pub fn must_refresh(&self) -> bool {
        match self {
            Self::Valid | Self::Stale => false,
            Self::Expired => true,
        }
    }
}
