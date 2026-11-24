//! UUIDv7 ids: time-ordered, sync-safe, sortable as strings.

/// A fresh UUIDv7 as a lowercase hyphenated string.
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// A short, speakable handle for an id: `#` plus its last six characters (for a
/// UUIDv7, six hex digits from the random tail). Shown to the model and in the
/// export; resolved back to an id by `Store::thread_by_handle`.
pub fn handle_of(id: &str) -> String {
    let tail: String = id
        .chars()
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("#{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_of_is_hash_plus_last_six() {
        assert_eq!(handle_of("0192f3a4-5b6c-7d8e-9f01-23456789a3f9"), "#89a3f9");
        assert_eq!(handle_of("abc"), "#abc");
    }

    #[test]
    fn new_id_is_a_uuid_v7() {
        let id = new_id();
        let parsed = uuid::Uuid::parse_str(&id).expect("valid uuid");
        assert_eq!(parsed.get_version_num(), 7);
    }

    #[test]
    fn ids_are_unique_and_time_ordered() {
        let a = new_id();
        let b = new_id();
        assert_ne!(a, b);
        assert!(a <= b, "uuid v7 string order follows creation order");
    }
}
