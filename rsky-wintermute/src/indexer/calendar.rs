use super::{sanitize_opt, sanitize_text};
use crate::types::WintermuteError;
use chrono::{DateTime, SecondsFormat, Utc};
use rsky_syntax::aturi::AtUri;

pub const EVENT_COLLECTION: &str = "community.lexicon.calendar.event";
pub const RSVP_COLLECTION: &str = "community.lexicon.calendar.rsvp";
const TID_ALPHABET: &[u8; 32] = b"234567abcdefghijklmnopqrstuvwxyz";

fn record_uri(did: &str, collection: &str, rkey: &str) -> Result<String, WintermuteError> {
    AtUri::new(format!("at://{did}/{collection}/{rkey}"), None)
        .map(|uri| uri.to_string())
        .map_err(|e| WintermuteError::Other(format!("invalid uri: {e}")))
}

fn format_time(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn normalize_datetime(value: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|time| format_time(time.with_timezone(&Utc)))
}

/// The creation time encoded in a TID record key.
pub fn tid_time(rkey: &str) -> Option<DateTime<Utc>> {
    if rkey.len() != 13 {
        return None;
    }
    let mut value: u64 = 0;
    for byte in rkey.bytes() {
        let digit = TID_ALPHABET.iter().position(|&c| c == byte)?;
        value = (value << 5) | u64::try_from(digit).ok()?;
    }
    if value >> 63 != 0 {
        return None;
    }
    DateTime::from_timestamp_micros(i64::try_from(value >> 10).ok()?)
}

fn str_field<'a>(record: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    record.get(key).and_then(serde_json::Value::as_str)
}

pub async fn index_event(
    client: &deadpool_postgres::Client,
    did: &str,
    rkey: &str,
    record: &serde_json::Value,
    cid: &str,
    indexed_at: &str,
) -> Result<(), WintermuteError> {
    let uri = record_uri(did, EVENT_COLLECTION, rkey)?;
    let starts_at = str_field(record, "startsAt").and_then(normalize_datetime);
    let status = sanitize_opt(str_field(record, "status"));

    client
        .execute(
            "INSERT INTO calendar_event (uri, cid, creator, \"startsAt\", status, \"indexedAt\")
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (uri) DO UPDATE SET
               cid = EXCLUDED.cid,
               \"startsAt\" = EXCLUDED.\"startsAt\",
               status = EXCLUDED.status,
               \"indexedAt\" = EXCLUDED.\"indexedAt\"",
            &[&uri, &cid, &did, &starts_at, &status, &indexed_at],
        )
        .await?;
    Ok(())
}

pub async fn delete_event(
    client: &deadpool_postgres::Client,
    did: &str,
    rkey: &str,
) -> Result<(), WintermuteError> {
    let uri = record_uri(did, EVENT_COLLECTION, rkey)?;
    client
        .execute("DELETE FROM calendar_event WHERE uri = $1", &[&uri])
        .await?;
    Ok(())
}

/// An RSVP's event and status, when the record is one this index can hold.
fn rsvp_target(record: &serde_json::Value) -> Option<(String, Option<String>, String)> {
    let subject = record.get("subject")?;
    let subject_uri = AtUri::new(sanitize_text(str_field(subject, "uri")?), None).ok()?;
    if subject_uri.get_collection() != EVENT_COLLECTION {
        return None;
    }
    let status = sanitize_text(str_field(record, "status")?);
    Some((
        subject_uri.to_string(),
        sanitize_opt(str_field(subject, "cid")),
        status,
    ))
}

pub async fn index_rsvp(
    client: &deadpool_postgres::Client,
    did: &str,
    rkey: &str,
    record: &serde_json::Value,
    cid: &str,
    indexed_at: &str,
) -> Result<(), WintermuteError> {
    let uri = record_uri(did, RSVP_COLLECTION, rkey)?;
    let Some((subject_uri, subject_cid, status)) = rsvp_target(record) else {
        // An update can turn a counted RSVP into one this index can't hold.
        client
            .execute("DELETE FROM calendar_rsvp WHERE uri = $1", &[&uri])
            .await?;
        return Ok(());
    };
    let sort_at = tid_time(rkey).map_or_else(|| indexed_at.to_owned(), format_time);

    client
        .execute(
            "DELETE FROM calendar_rsvp WHERE uri = $1 AND subject <> $2",
            &[&uri, &subject_uri],
        )
        .await?;
    client
        .execute(
            "INSERT INTO calendar_rsvp (creator, subject, uri, cid, \"subjectCid\", status, \"indexedAt\", \"sortAt\")
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (creator, subject) DO UPDATE SET
               uri = EXCLUDED.uri,
               cid = EXCLUDED.cid,
               \"subjectCid\" = EXCLUDED.\"subjectCid\",
               status = EXCLUDED.status,
               \"indexedAt\" = EXCLUDED.\"indexedAt\",
               \"sortAt\" = EXCLUDED.\"sortAt\"",
            &[&did, &subject_uri, &uri, &cid, &subject_cid, &status, &indexed_at, &sort_at],
        )
        .await?;
    Ok(())
}

pub async fn delete_rsvp(
    client: &deadpool_postgres::Client,
    did: &str,
    rkey: &str,
) -> Result<(), WintermuteError> {
    let uri = record_uri(did, RSVP_COLLECTION, rkey)?;
    client
        .execute("DELETE FROM calendar_rsvp WHERE uri = $1", &[&uri])
        .await?;
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::{normalize_datetime, record_uri, tid_time};
    use chrono::{DateTime, Utc};

    pub fn tid_at(time: DateTime<Utc>, clock: u64) -> String {
        let alphabet = super::TID_ALPHABET;
        let mut value = (u64::try_from(time.timestamp_micros()).unwrap() << 10) | clock;
        let mut out = [0u8; 13];
        for slot in out.iter_mut().rev() {
            *slot = alphabet[usize::try_from(value & 31).unwrap()];
            value >>= 5;
        }
        String::from_utf8(out.to_vec()).unwrap()
    }

    #[test]
    fn decodes_tid_time() {
        let time = DateTime::parse_from_rfc3339("2026-09-26T12:34:56.789Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(tid_time(&tid_at(time, 7)), Some(time));
        assert_eq!(tid_time("3kxyz"), None);
        assert_eq!(tid_time("!!!!!!!!!!!!!"), None);
        assert_eq!(tid_time("zzzzzzzzzzzzz"), None);
    }

    #[test]
    fn rejects_malformed_record_uris() {
        assert!(record_uri("not a did", super::EVENT_COLLECTION, "rkey").is_err());
    }

    #[test]
    fn normalizes_datetimes_to_utc_millis() {
        assert_eq!(
            normalize_datetime("2026-10-01T19:00:00-04:00").as_deref(),
            Some("2026-10-01T23:00:00.000Z")
        );
        assert_eq!(normalize_datetime("next tuesday"), None);
    }
}
