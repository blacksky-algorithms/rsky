use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::de::Error as _;
use std::collections::HashMap;

/// Creates a cached getter method for a struct field.
///
/// This trait is used to implement a caching decorator similar to TypeScript.
pub trait CachedGetter<T> {
    fn get_cached(&self) -> &T;
}

/// Represents a match pattern helper for filtering values.
pub fn matches_any<T>(value: Option<T>) -> Box<dyn Fn(&T) -> bool>
where
    T: PartialEq + 'static,
{
    Box::new(move |v| match &value {
        Some(t) => t == v,
        None => true,
    })
}

/// Compares items based on a preferred order list.
pub fn preferred_order_cmp<T>(order: &[T]) -> impl Fn(&T, &T) -> std::cmp::Ordering + '_
where
    T: PartialEq,
{
    move |a, b| {
        let a_idx = order.iter().position(|x| x == a);
        let b_idx = order.iter().position(|x| x == b);
        match (a_idx, b_idx) {
            (None, None) => std::cmp::Ordering::Equal,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(_), None) => std::cmp::Ordering::Less,
            (Some(a), Some(b)) => a.cmp(&b),
        }
    }
}

/// Filter defined values from an iterator, similar to TypeScript's isDefined.
pub fn is_defined<T>(x: &Option<T>) -> bool {
    x.is_some()
}

/// Helper trait to filter map entries from an iterator.
pub trait FilterMap<K, V> {
    fn filter_map_values<F, U>(self, f: F) -> HashMap<K, U>
    where
        F: FnMut(&V) -> Option<U>,
        K: std::hash::Hash + Eq + Clone;
}

impl<K, V> FilterMap<K, V> for HashMap<K, V> {
    fn filter_map_values<F, U>(self, mut f: F) -> HashMap<K, U>
    where
        F: FnMut(&V) -> Option<U>,
        K: std::hash::Hash + Eq + Clone,
    {
        self.iter()
            .filter_map(|(k, v)| f(v).map(|new_v| (k.clone(), new_v)))
            .collect()
    }
}

/// Parse base64 URL-encoded JSON into a value.
pub fn parse_b64u_json<T>(input: &str) -> Result<T, serde_json::Error>
where
    T: serde::de::DeserializeOwned,
{
    let decoded = URL_SAFE_NO_PAD
        .decode(input)
        .map_err(|_| serde_json::Error::custom("Invalid base64url encoding"))?;

    let json_str = String::from_utf8(decoded)
        .map_err(|_| serde_json::Error::custom("Invalid UTF-8 encoding"))?;

    serde_json::from_str(&json_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_any() {
        let matcher = matches_any(Some(5));
        assert!(matcher(&5));
        assert!(!matcher(&6));

        let matcher_none = matches_any::<i32>(None);
        assert!(matcher_none(&5));
        assert!(matcher_none(&6));
    }

    #[test]
    fn test_preferred_order_cmp() {
        let order = vec![1, 2, 3];
        let cmp = preferred_order_cmp(&order);

        assert_eq!(cmp(&1, &2), std::cmp::Ordering::Less);
        assert_eq!(cmp(&2, &1), std::cmp::Ordering::Greater);
        assert_eq!(cmp(&4, &5), std::cmp::Ordering::Equal);
        assert_eq!(cmp(&1, &4), std::cmp::Ordering::Less);
        assert_eq!(cmp(&4, &1), std::cmp::Ordering::Greater);
    }

    #[test]
    fn test_filter_map_values() {
        let mut map = HashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);

        let result = map.filter_map_values(|v| if *v > 1 { Some(v * 2) } else { None });

        assert_eq!(result.len(), 2);
        assert_eq!(result.get("b"), Some(&4));
        assert_eq!(result.get("c"), Some(&6));
        assert_eq!(result.get("a"), None);
    }

    #[test]
    fn test_parse_b64u_json() {
        let json = r#"{"key":"value"}"#;

        // Use modern API for encoding in test
        let encoded = URL_SAFE_NO_PAD.encode(json);

        let result: HashMap<String, String> = parse_b64u_json(&encoded).unwrap();

        assert_eq!(result.get("key"), Some(&"value".to_string()));
    }
}
