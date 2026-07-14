// based on https://github.com/bluesky-social/atproto/blob/main/packages/pds/src/db/pagination.ts

use anyhow::{bail, Result};
use chrono::{DateTime, SecondsFormat, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    pub primary: String,
    pub secondary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

pub fn pack_cursor(cursor: Option<Cursor>) -> Option<String> {
    cursor.map(|cursor| format!("{}::{}", cursor.primary, cursor.secondary))
}

pub fn unpack_cursor(cursor_str: Option<&str>) -> Result<Option<Cursor>> {
    let Some(cursor_str) = cursor_str else {
        return Ok(None);
    };
    let parts: Vec<&str> = cursor_str.split("::").collect();
    match parts.as_slice() {
        [primary, secondary] if !primary.is_empty() && !secondary.is_empty() => Ok(Some(Cursor {
            primary: (*primary).to_string(),
            secondary: (*secondary).to_string(),
        })),
        _ => bail!("Malformed cursor"),
    }
}

/// Keyset over a (sortAt/createdAt, cid) pair where the packed cursor encodes
/// the primary value as unix millis.
#[derive(Debug, Clone)]
pub struct TimeCidKeyset {
    pub primary_column: String,
    pub secondary_column: String,
}

impl TimeCidKeyset {
    pub fn new(primary_column: impl Into<String>, secondary_column: impl Into<String>) -> Self {
        TimeCidKeyset {
            primary_column: primary_column.into(),
            secondary_column: secondary_column.into(),
        }
    }

    /// Packs the last row of a result set into a cursor string.
    pub fn pack_from_result(&self, results: &[(String, String)]) -> Result<Option<String>> {
        let Some((created_at, cid)) = results.last() else {
            return Ok(None);
        };
        let millis = parse_datetime_to_millis(created_at)?;
        Ok(pack_cursor(Some(Cursor {
            primary: millis.to_string(),
            secondary: cid.clone(),
        })))
    }

    /// Unpacks a cursor string into (datetime string, cid) values usable in a query.
    pub fn unpack(&self, cursor_str: Option<&str>) -> Result<Option<(String, String)>> {
        let Some(cursor) = unpack_cursor(cursor_str)? else {
            return Ok(None);
        };
        let Ok(millis) = cursor.primary.parse::<i64>() else {
            bail!("Malformed cursor")
        };
        let Some(datetime) = DateTime::<Utc>::from_timestamp_millis(millis) else {
            bail!("Malformed cursor")
        };
        Ok(Some((
            datetime.to_rfc3339_opts(SecondsFormat::Millis, true),
            cursor.secondary,
        )))
    }

    /// A row-value WHERE fragment with two positional placeholders for
    /// (primary, secondary), written to hit an index on those columns.
    pub fn where_clause(&self, direction: SortDirection) -> String {
        let op = match direction {
            SortDirection::Asc => ">",
            SortDirection::Desc => "<",
        };
        format!(
            "(({}, {}) {op} (?, ?))",
            self.primary_column, self.secondary_column
        )
    }

    pub fn order_by_clause(&self, direction: SortDirection) -> String {
        let dir = match direction {
            SortDirection::Asc => "ASC",
            SortDirection::Desc => "DESC",
        };
        format!(
            "{} {dir}, {} {dir}",
            self.primary_column, self.secondary_column
        )
    }
}

/// Keyset over a single rkey column where the cursor is the rkey itself.
#[derive(Debug, Clone)]
pub struct RkeyKeyset {
    pub column: String,
}

impl RkeyKeyset {
    pub fn new(column: impl Into<String>) -> Self {
        RkeyKeyset {
            column: column.into(),
        }
    }

    pub fn pack_from_result(&self, results: &[String]) -> Option<String> {
        results.last().cloned()
    }

    /// A WHERE fragment with one positional placeholder for the rkey cursor.
    pub fn where_clause(&self, direction: SortDirection) -> String {
        let op = match direction {
            SortDirection::Asc => ">",
            SortDirection::Desc => "<",
        };
        format!("({} {op} ?)", self.column)
    }

    pub fn order_by_clause(&self, direction: SortDirection) -> String {
        let dir = match direction {
            SortDirection::Asc => "ASC",
            SortDirection::Desc => "DESC",
        };
        format!("{} {dir}", self.column)
    }
}

fn parse_datetime_to_millis(datetime: &str) -> Result<i64> {
    match DateTime::parse_from_rfc3339(datetime) {
        Ok(parsed) => Ok(parsed.timestamp_millis()),
        Err(_) => bail!("Malformed datetime: {datetime}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_and_unpacks_cursors() {
        let packed = pack_cursor(Some(Cursor {
            primary: "123".to_owned(),
            secondary: "bafy".to_owned(),
        }));
        assert_eq!(packed.as_deref(), Some("123::bafy"));
        let unpacked = unpack_cursor(packed.as_deref()).unwrap().unwrap();
        assert_eq!(unpacked.primary, "123");
        assert_eq!(unpacked.secondary, "bafy");
        assert_eq!(pack_cursor(None), None);
        assert_eq!(unpack_cursor(None).unwrap(), None);
    }

    #[test]
    fn rejects_malformed_cursors() {
        assert!(unpack_cursor(Some("no-separator")).is_err());
        assert!(unpack_cursor(Some("a::b::c")).is_err());
        assert!(unpack_cursor(Some("::missing")).is_err());
        assert!(unpack_cursor(Some("missing::")).is_err());
    }

    #[test]
    fn time_cid_keyset_round_trips() {
        let keyset = TimeCidKeyset::new("record.createdAt", "record.cid");
        let rows = vec![
            ("2023-01-01T00:00:00.000Z".to_owned(), "bafyone".to_owned()),
            ("2023-01-02T03:04:05.678Z".to_owned(), "bafytwo".to_owned()),
        ];
        let cursor = keyset.pack_from_result(&rows).unwrap().unwrap();
        assert_eq!(cursor, format!("{}::bafytwo", 1672628645678i64));
        let (datetime, cid) = keyset.unpack(Some(&cursor)).unwrap().unwrap();
        assert_eq!(datetime, "2023-01-02T03:04:05.678Z");
        assert_eq!(cid, "bafytwo");
    }

    #[test]
    fn time_cid_keyset_handles_empty_and_missing() {
        let keyset = TimeCidKeyset::new("createdAt", "cid");
        assert_eq!(keyset.pack_from_result(&[]).unwrap(), None);
        assert_eq!(keyset.unpack(None).unwrap(), None);
    }

    #[test]
    fn time_cid_keyset_rejects_bad_input() {
        let keyset = TimeCidKeyset::new("createdAt", "cid");
        assert!(keyset
            .pack_from_result(&[("not-a-date".to_owned(), "bafy".to_owned())])
            .is_err());
        assert!(keyset.unpack(Some("not-millis::bafy")).is_err());
        assert!(keyset.unpack(Some(&format!("{}::bafy", i64::MAX))).is_err());
    }

    #[test]
    fn time_cid_keyset_sql_fragments() {
        let keyset = TimeCidKeyset::new("createdAt", "cid");
        assert_eq!(
            keyset.where_clause(SortDirection::Desc),
            "((createdAt, cid) < (?, ?))"
        );
        assert_eq!(
            keyset.where_clause(SortDirection::Asc),
            "((createdAt, cid) > (?, ?))"
        );
        assert_eq!(
            keyset.order_by_clause(SortDirection::Desc),
            "createdAt DESC, cid DESC"
        );
        assert_eq!(
            keyset.order_by_clause(SortDirection::Asc),
            "createdAt ASC, cid ASC"
        );
    }

    #[test]
    fn rkey_keyset_fragments_and_cursor() {
        let keyset = RkeyKeyset::new("rkey");
        assert_eq!(keyset.where_clause(SortDirection::Desc), "(rkey < ?)");
        assert_eq!(keyset.where_clause(SortDirection::Asc), "(rkey > ?)");
        assert_eq!(keyset.order_by_clause(SortDirection::Desc), "rkey DESC");
        assert_eq!(keyset.order_by_clause(SortDirection::Asc), "rkey ASC");
        assert_eq!(
            keyset.pack_from_result(&["aaa".to_owned(), "bbb".to_owned()]),
            Some("bbb".to_owned())
        );
        assert_eq!(keyset.pack_from_result(&[]), None);
    }
}
