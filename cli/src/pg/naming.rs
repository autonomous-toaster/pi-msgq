//! How an address becomes a queue name.
//!
//! pgmq derives table names by lowercasing and prefixing, and caps them, so an address cannot be
//! passed through: session/Api and session/api would land in one queue and a path-like address would be
//! rejected. The name is therefore derived - a readable slug plus a digest - and the true address
//! is kept in msgq.address and in every record's headers.

use sha2::{Digest, Sha256};

/// pgmq refuses queue names longer than this, because the table name carries a prefix.
const QUEUE_NAME_MAX: usize = 47;

/// The derived queue name for an address: a readable slug plus a digest.
///
/// The digest is the identity — the slug exists so `\dt pgmq.*` stays legible — and the pair is
/// capped where pgmq caps it. Lowercasing is safe because the digest, not the slug, distinguishes
/// addresses.
pub fn queue_name(address: &str) -> String {
    let digest = {
        let mut hasher = Sha256::new();
        hasher.update(address.as_bytes());
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    let slug: String = address
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let suffix = &digest[..16];
    let room = QUEUE_NAME_MAX.saturating_sub(suffix.len() + 1);
    let mut slug = slug.trim_matches('_').to_string();
    slug.truncate(room);
    let slug = slug.trim_end_matches('_');
    format!("{slug}_{suffix}").to_lowercase()
}

/// Quote an identifier for interpolation, refusing anything that is not one.
pub(super) fn quote_ident(raw: &str) -> String {
    debug_assert!(
        raw.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.'),
        "names are derived, so an unsafe identifier is a bug: {raw}"
    );
    format!("\"{raw}\"")
}

/// A derived name always matches the identifier alphabet, so it can be interpolated inside a string
/// literal for the pgmq functions that take a queue name as text.
pub(super) fn escaped(name: &str) -> String {
    debug_assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    name.replace('\'', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_queue_name_is_bounded_and_stable() {
        let long = format!("session/{}", "very-long-path-segment-".repeat(6));
        let name = queue_name(&long);
        assert!(
            name.len() <= QUEUE_NAME_MAX,
            "{name} is {} long",
            name.len()
        );
        assert_eq!(name, queue_name(&long), "the name is derived, not random");
    }

    #[test]
    fn case_differing_addresses_get_different_names() {
        // pgmq lowercases table names, so the digest has to carry the distinction.
        assert_ne!(queue_name("session/Api"), queue_name("session/api"));
    }

    #[test]
    fn colliding_slugs_still_differ() {
        // Same readable portion, different address: only the digest can separate them.
        let a = "session/a.b";
        let b = "session/a-b";
        assert_ne!(queue_name(a), queue_name(b));
        assert!(queue_name(a).starts_with("session_a"));
    }
}
