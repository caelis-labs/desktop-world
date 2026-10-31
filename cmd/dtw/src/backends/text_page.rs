//! UTF-16 paging shared with native providers; offsets are never byte indices.
use crate::computer::{Error, TextPage};

pub fn matches_fields(label: &str, name: &str, value: &str, lowercase_needle: &str) -> bool {
    [label, name, value]
        .iter()
        .any(|field| field.to_lowercase().contains(lowercase_needle))
}

pub fn page(text: &str, offset: u32, limit: u32) -> Result<TextPage, Error> {
    if !(2..=16_384).contains(&limit) {
        return Err(Error::InvalidObservation);
    }
    let units: Vec<u16> = text.encode_utf16().collect();
    let start = offset as usize;
    if start > units.len()
        || (start > 0 && start < units.len() && (0xDC00..=0xDFFF).contains(&units[start]))
    {
        return Err(Error::NativeFailure{code:"invalid_text_range".into(),message:"Offset must be a valid UTF-16 boundary within the retained full text; reuse next_offset from the preceding page".into()});
    }
    let mut end = (start + limit as usize).min(units.len());
    if end < units.len() && (0xDC00..=0xDFFF).contains(&units[end]) {
        end -= 1;
    }
    Ok(TextPage {
        text: String::from_utf16(&units[start..end]).map_err(|_| Error::InvalidObservation)?,
        offset,
        next_offset: (end < units.len()).then_some(end as u32),
        total_length: units.len() as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_matching_does_not_search_truncated_discovery_previews() {
        let full = format!("{}Needle", "x".repeat(800));
        assert!(matches_fields("", &full, "", "needle"));
        assert!(matches_fields("", "", &full, "needle"));
        assert!(!matches_fields("", &full, "", "absent"));
    }
    #[test]
    fn search_cannot_match_across_distinct_native_fields() {
        assert!(!matches_fields(
            "text_field Order",
            "Order",
            "Number",
            "order number"
        ));
    }
    #[test]
    fn search_accepts_the_published_label_and_each_native_field() {
        for needle in ["button save", "save", "ready"] {
            assert!(matches_fields("button Save", "Save", "Ready", needle));
        }
        assert!(!matches_fields(
            "button Save",
            "Save",
            "Ready",
            "save ready"
        ));
    }
    #[test]
    fn utf16_pages_preserve_surrogates_and_empty_terminal_page() {
        let first = page("a😀b", 0, 2).unwrap();
        assert_eq!(first.text, "a");
        assert_eq!(first.next_offset, Some(1));
        let second = page("a😀b", 1, 2).unwrap();
        assert_eq!(second.text, "😀");
        assert_eq!(second.next_offset, Some(3));
        assert_eq!(page("a😀b", 3, 2).unwrap().text, "b");
        assert_eq!(page("a😀b", 4, 2).unwrap().next_offset, None);
        assert!(page("a😀b", 2, 2).is_err());
        assert!(page("a", 2, 2).is_err());
        assert!(page("", 0, 1).is_err());
    }
}
