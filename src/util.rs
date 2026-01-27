//! Utility functions for bean ID parsing and status conversion.

use crate::bean::Status;
use std::str::FromStr;

/// Compare two bean IDs using natural ordering.
/// Parses IDs as dot-separated numeric segments and compares lexicographically.
///
/// # Examples
/// - "1" < "2" (numeric comparison)
/// - "1" < "10" (numeric comparison, not string comparison)
/// - "3.1" < "3.2" (multi-level comparison)
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let sa = parse_id_segments(a);
    let sb = parse_id_segments(b);
    sa.cmp(&sb)
}

/// Parse a dot-separated ID into numeric segments.
///
/// Each segment is parsed as u64. Non-numeric segments are skipped.
/// Used for natural ID comparison.
///
/// # Examples
/// - "1" → [1]
/// - "3.1" → [3, 1]
/// - "3.2.1" → [3, 2, 1]
fn parse_id_segments(id: &str) -> Vec<u64> {
    id.split('.')
        .filter_map(|seg| seg.parse::<u64>().ok())
        .collect()
}

/// Convert a status string to a Status enum, or None if invalid.
///
/// Valid inputs: "open", "in_progress", "closed"
pub fn parse_status(s: &str) -> Option<Status> {
    match s {
        "open" => Some(Status::Open),
        "in_progress" => Some(Status::InProgress),
        "closed" => Some(Status::Closed),
        _ => None,
    }
}

/// Implement FromStr for Status to support standard parsing.
impl FromStr for Status {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_status(s).ok_or_else(|| format!("Invalid status: {}", s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- natural_cmp tests ----------

    #[test]
    fn natural_cmp_single_digit() {
        assert_eq!(natural_cmp("1", "2"), std::cmp::Ordering::Less);
        assert_eq!(natural_cmp("2", "1"), std::cmp::Ordering::Greater);
        assert_eq!(natural_cmp("1", "1"), std::cmp::Ordering::Equal);
    }

    #[test]
    fn natural_cmp_multi_digit() {
        assert_eq!(natural_cmp("1", "10"), std::cmp::Ordering::Less);
        assert_eq!(natural_cmp("10", "1"), std::cmp::Ordering::Greater);
        assert_eq!(natural_cmp("10", "10"), std::cmp::Ordering::Equal);
    }

    #[test]
    fn natural_cmp_multi_level() {
        assert_eq!(natural_cmp("3.1", "3.2"), std::cmp::Ordering::Less);
        assert_eq!(natural_cmp("3.2", "3.1"), std::cmp::Ordering::Greater);
        assert_eq!(natural_cmp("3.1", "3.1"), std::cmp::Ordering::Equal);
    }

    #[test]
    fn natural_cmp_three_level() {
        assert_eq!(natural_cmp("3.2.1", "3.2.2"), std::cmp::Ordering::Less);
        assert_eq!(natural_cmp("3.2.2", "3.2.1"), std::cmp::Ordering::Greater);
        assert_eq!(natural_cmp("3.2.1", "3.2.1"), std::cmp::Ordering::Equal);
    }

    #[test]
    fn natural_cmp_different_prefix() {
        assert_eq!(natural_cmp("2.1", "3.1"), std::cmp::Ordering::Less);
        assert_eq!(natural_cmp("10.5", "9.99"), std::cmp::Ordering::Greater);
    }

    // ---------- parse_id_segments tests ----------

    #[test]
    fn parse_id_segments_single() {
        assert_eq!(parse_id_segments("1"), vec![1]);
        assert_eq!(parse_id_segments("42"), vec![42]);
    }

    #[test]
    fn parse_id_segments_multi_level() {
        assert_eq!(parse_id_segments("1.2"), vec![1, 2]);
        assert_eq!(parse_id_segments("3.2.1"), vec![3, 2, 1]);
    }

    #[test]
    fn parse_id_segments_leading_zeros() {
        // Leading zeros are parsed as decimal, not octal
        assert_eq!(parse_id_segments("01"), vec![1]);
        assert_eq!(parse_id_segments("03.02"), vec![3, 2]);
    }

    #[test]
    fn parse_id_segments_non_numeric_skipped() {
        let empty: Vec<u64> = vec![];
        assert_eq!(parse_id_segments("abc"), empty);
        assert_eq!(parse_id_segments("1.abc.2"), vec![1, 2]);
    }

    // ---------- parse_status tests ----------

    #[test]
    fn parse_status_valid_open() {
        assert_eq!(parse_status("open"), Some(Status::Open));
    }

    #[test]
    fn parse_status_valid_in_progress() {
        assert_eq!(parse_status("in_progress"), Some(Status::InProgress));
    }

    #[test]
    fn parse_status_valid_closed() {
        assert_eq!(parse_status("closed"), Some(Status::Closed));
    }

    #[test]
    fn parse_status_invalid() {
        assert_eq!(parse_status("invalid"), None);
        assert_eq!(parse_status(""), None);
        assert_eq!(parse_status("OPEN"), None);
        assert_eq!(parse_status("Closed"), None);
    }

    #[test]
    fn parse_status_whitespace() {
        assert_eq!(parse_status("open "), None);
        assert_eq!(parse_status(" open"), None);
    }

    // ---------- Status::FromStr tests ----------

    #[test]
    fn status_from_str_open() {
        assert_eq!("open".parse::<Status>(), Ok(Status::Open));
    }

    #[test]
    fn status_from_str_in_progress() {
        assert_eq!("in_progress".parse::<Status>(), Ok(Status::InProgress));
    }

    #[test]
    fn status_from_str_closed() {
        assert_eq!("closed".parse::<Status>(), Ok(Status::Closed));
    }

    #[test]
    fn status_from_str_invalid() {
        assert!("invalid".parse::<Status>().is_err());
        assert!("".parse::<Status>().is_err());
    }
}
