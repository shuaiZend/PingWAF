//! URL decoding helpers.
//!
//! `urlencoding::decode` handles a single pass; WAF input is frequently
//! double- or triple-encoded to slip past naive filters, so we keep decoding
//! until the value stops changing (bounded by `max_layers`).

/// Apply percent-decoding repeatedly until the string is stable or the layer
/// budget is exhausted. Returns the input unchanged when there is nothing to
/// decode.
///
/// Invalid UTF-8 sequences halt the loop and return the most recent valid
/// form rather than failing — the WAF still has the previous layer to scan.
pub fn multi_decode(input: &str, max_layers: usize) -> String {
    if max_layers == 0 || !input.contains('%') {
        return input.to_string();
    }
    let mut current = input.to_string();
    for _ in 0..max_layers {
        let decoded = match urlencoding::decode(&current) {
            Ok(cow) => cow.into_owned(),
            Err(_) => break,
        };
        if decoded == current {
            break;
        }
        current = decoded;
        if !current.contains('%') {
            break;
        }
    }
    current
}

/// Lower-case every percent-encoded triplet in `input` so that `%2E` and `%2e`
/// compare equal in subsequent signature matching. Does NOT decode anything.
pub fn normalize_percent_case(input: &str) -> String {
    if !input.contains('%') {
        return input.to_string();
    }
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'%' && i + 2 < bytes.len() {
            let hi = bytes[i + 1];
            let lo = bytes[i + 2];
            if hi.is_ascii_hexdigit() && lo.is_ascii_hexdigit() {
                out.push('%');
                out.push((hi as char).to_ascii_lowercase());
                out.push((lo as char).to_ascii_lowercase());
                i += 3;
                continue;
            }
        }
        out.push(c as char);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_decode() {
        assert_eq!(multi_decode("hello%20world", 3), "hello world");
    }

    #[test]
    fn double_decode() {
        assert_eq!(multi_decode("%252e%252e%252f", 3), "../");
    }

    #[test]
    fn triple_decode() {
        assert_eq!(multi_decode("%25252e%25252e%25252f", 4), "../");
    }

    #[test]
    fn stops_at_layer_budget() {
        // Two passes needed; with budget 1 we should still see one decoded form.
        assert_eq!(multi_decode("%252e", 1), "%2e");
    }

    #[test]
    fn no_percent_is_noop() {
        assert_eq!(multi_decode("plain text", 3), "plain text");
    }

    #[test]
    fn lowercases_percent_triplets() {
        assert_eq!(normalize_percent_case("%2E%2e%2F"), "%2e%2e%2f");
    }
}
