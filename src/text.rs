//! Boundary-safe text utilities (UTF-8 aware).
//!
//! Byte-index slicing (`&s[..n]`) panics when `n` falls inside a multi-byte
//! UTF-8 character (Persian/Arabic = 2 bytes, CJK/emoji = 3–4 bytes) — the
//! recurring crash class tracked in nghr f844d2df (and its predecessor
//! 811ed903 in trustee). All truncation of potentially non-ASCII text
//! (task descriptions, user commands, LLM-generated titles, agent names)
//! must go through the helpers in this module.
//!
//! This module is deliberately dependency-free and always compiled (no
//! feature gate) so both `checkpoint` and `cli` code paths can use it.

/// Truncate `s` to at most `max` bytes, appending `"..."` when truncated.
///
/// Never panics on any UTF-8 input: the cut lands on the last char boundary
/// at or before `max - 3` bytes. For pure-ASCII input the output is
/// byte-for-byte identical to the legacy idiom
/// `if s.len() > max { format!("{}...", &s[..max - 3]) } else { s }`,
/// which matters because the session-description / re-title probe in the
/// CLI runner compares this string against descriptions stored by older
/// abk versions (nghr f844d2df, finding F3).
pub fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let cut = max.saturating_sub(3);
        let end = s
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= cut)
            .last()
            .unwrap_or(0);
        format!("{}...", &s[..end])
    }
}

/// Return the longest prefix of `s` that is at most `max_bytes` long.
///
/// Boundary-safe for any UTF-8 input: the returned slice always ends on a
/// char boundary (or is empty). Callers append their own suffix/ellipsis so
/// this stays usable for non-"..." markers (e.g. trustee's
/// `"... [truncated]"` history marker).
pub fn truncate_at_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let end = s
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| i <= max_bytes)
        .last()
        .unwrap_or(0);
    &s[..end]
}

/// Uppercase the first character of `s`, leaving the rest untouched.
///
/// Boundary-safe replacement for `first.to_uppercase() + &s[1..]`, which
/// panics when the first character is multi-byte (e.g. a Persian agent
/// name). Empty input returns an empty string (no panic on `unwrap`).
pub fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Byte-level SSE event buffer that never splits multi-byte UTF-8 sequences.
///
/// SSE events are framed by the ASCII separator `\n\n`. Because `\n` is a
/// single-byte character it can never occur *inside* a multi-byte UTF-8
/// sequence, so every `\n\n` boundary is a valid char boundary — decoding
/// complete events only makes it impossible to corrupt a character that
/// straddles an HTTP/TCP chunk edge (nghr b33d3efc: applying
/// `String::from_utf8_lossy` to each raw chunk turned such characters into
/// U+FFFD replacement glyphs, silently damaging Persian/emoji output at
/// random offsets).
pub struct SseBuffer {
    buf: Vec<u8>,
}

impl Default for SseBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl SseBuffer {
    /// Create an empty buffer.
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Append a raw HTTP chunk. May split events and characters anywhere;
    /// both are reassembled internally before decoding.
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Pop the next complete event (without its `\n\n` separator), if any.
    ///
    /// Decoding happens only over complete events, so a multi-byte
    /// character split across two chunks is reassembled at the byte level
    /// before it ever becomes a `String`.
    pub fn next_event(&mut self) -> Option<String> {
        let end = self.buf.windows(2).position(|w| w == b"\n\n")?;
        let event: Vec<u8> = self.buf.drain(..end + 2).collect();
        let cut = event.len() - 2; // drop the "\n\n" separator
        Some(decode_utf8_prefer_strict(&event[..cut]))
    }

    /// Decode and clear whatever remains. Call at stream end when the final
    /// event may lack its trailing separator. A truncated final character
    /// (genuinely broken stream) still degrades to U+FFFD — by then the
    /// data is unrecoverable, matching the old behavior for garbage input.
    pub fn flush(&mut self) -> Option<String> {
        if self.buf.is_empty() {
            return None;
        }
        let rest = std::mem::take(&mut self.buf);
        Some(decode_utf8_prefer_strict(&rest))
    }

    /// True when nothing is buffered.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

/// Strict UTF-8 decode with lossy fallback (never panics; valid input is
/// byte-identical, invalid input degrades exactly like `from_utf8_lossy`).
fn decode_utf8_prefer_strict(bytes: &[u8]) -> String {
    match String::from_utf8(bytes.to_vec()) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- truncate_str: ASCII legacy identity (the F3 compat contract) ---

    #[test]
    fn ascii_long_truncates_identically_to_legacy() {
        let s = "A".repeat(100);
        assert_eq!(truncate_str(&s, 80), format!("{}...", "A".repeat(77)));
        assert_eq!(truncate_str(&s, 50), format!("{}...", "A".repeat(47)));
    }

    #[test]
    fn ascii_exactly_at_threshold_unchanged() {
        let s80 = "A".repeat(80);
        assert_eq!(truncate_str(&s80, 80), s80);
        let s50 = "A".repeat(50);
        assert_eq!(truncate_str(&s50, 50), s50);
    }

    // --- truncate_str: the reported crash class ---

    #[test]
    fn persian_long_does_not_panic_and_cuts_at_boundary() {
        // 100 Persian chars = 200 bytes; legacy &s[..77] panicked here
        // (`end byte index 77 is not a char boundary`).
        let s = "ن".repeat(100);
        let out = truncate_str(&s, 80);
        assert!(out.ends_with("..."));
        assert_eq!(out, format!("{}...", "ن".repeat(38))); // 38×2=76 ≤ 77
    }

    #[test]
    fn persian_under_byte_threshold_unchanged() {
        let s = "سلام دنیا".to_string(); // 18 bytes
        assert_eq!(truncate_str(&s, 80), s);
    }

    #[test]
    fn four_byte_char_straddling_cut_is_excluded_not_sliced() {
        // 20×4-byte chars cover bytes 0..80; the 20th straddles the 77 cut.
        let s = "\u{20BB7}".repeat(100);
        let out = truncate_str(&s, 80);
        assert_eq!(out, format!("{}...", "\u{20BB7}".repeat(19))); // 19×4=76
    }

    #[test]
    fn boundary_exactly_at_cut_same_as_legacy() {
        let s = format!("{}{}", "A".repeat(77), "BCDE");
        assert_eq!(truncate_str(&s, 80), format!("{}...", "A".repeat(77)));
    }

    #[test]
    fn mixed_scripts_never_panic_and_stay_under_budget() {
        let s = format!("{}{}{}", "abc ".repeat(10), "نص فارسی ".repeat(20), "🙂".repeat(30));
        let out = truncate_str(&s, 80);
        assert!(out.ends_with("..."));
        assert!(out.len() <= 80, "body must stay within the byte budget, got {}", out.len());
        assert!(out.is_char_boundary(out.len() - 3));
    }

    #[test]
    fn empty_and_single_char_unchanged() {
        assert_eq!(truncate_str("", 80), "");
        assert_eq!(truncate_str("x", 80), "x");
    }

    #[test]
    fn tiny_max_does_not_underflow_or_panic() {
        assert_eq!(truncate_str("نننن", 2), "...");
        assert_eq!(truncate_str("abc", 0), "...");
    }

    // --- truncate_at_boundary ---

    #[test]
    fn truncate_at_boundary_ascii_is_exact() {
        assert_eq!(truncate_at_boundary("abcdefghij", 4), "abcd");
        assert_eq!(truncate_at_boundary("abc", 4), "abc");
        assert_eq!(truncate_at_boundary("", 4), "");
    }

    #[test]
    fn truncate_at_boundary_multibyte_cuts_clean() {
        let s = "ب".repeat(6000); // 12,000 bytes; byte 10,000 is mid-char parity-dependent
        let cut = truncate_at_boundary(&s, 10_000);
        assert!(cut.len() <= 10_000);
        assert!(cut.chars().count() >= 4_998); // never lose more than one char
        assert!(s.starts_with(cut));
    }

    // --- capitalize_first ---

    #[test]
    fn capitalize_first_ascii() {
        assert_eq!(capitalize_first("ravand"), "Ravand");
    }

    #[test]
    fn capitalize_first_persian_no_panic() {
        // `&s[1..]` on this input panics at byte 1 (mid-character).
        assert_eq!(capitalize_first("رَوَند"), "رَوَند".to_uppercase()); // no-op for Persian
        let upper_first: String = 'ر'.to_uppercase().collect();
        assert_eq!(capitalize_first("رabc"), format!("{}abc", upper_first));
    }

    #[test]
    fn capitalize_first_empty() {
        assert_eq!(capitalize_first(""), "");
    }

    // --- SseBuffer: the nghr b33d3efc corruption class ---

    fn sse_frame(payload: &str) -> String {
        format!("{}\n\n", payload)
    }

    /// THE regression test: a ZWNJ-dense Persian event fed in chunks of
    /// every size 1..=8 must reassemble byte-identically. The old
    /// per-chunk `from_utf8_lossy` corrupted this at essentially every
    /// split point.
    #[test]
    fn sse_multibyte_survives_every_chunk_split() {
        let payload = "قابل‌اندازه‌گیری پیشنهاد بازنگری‌شده 🙂کتاب‌خانه می‌رود خانه‌ی";
        let stream = sse_frame(&format!("data: {}", payload));
        for size in 1..=8usize {
            let mut b = SseBuffer::new();
            let mut events = Vec::new();
            for chunk in stream.as_bytes().chunks(size) {
                b.push(chunk);
                while let Some(ev) = b.next_event() {
                    events.push(ev);
                }
            }
            assert_eq!(
                events,
                vec![format!("data: {}", payload)],
                "corruption at chunk size {}",
                size
            );
            assert!(b.is_empty(), "buffer not drained at chunk size {}", size);
        }
    }

    #[test]
    fn sse_full_stream_multiple_events_roundtrip() {
        let stream = format!(
            "{}\n\n{}\n\n{}\n\n",
            "data: {\"delta\":\"سلام دنیا\"}",
            "data: {\"delta\":\"نیم‌فاصله و ZWNJ‌تست 🙂\"}",
            "data: [DONE]"
        );
        let mut b = SseBuffer::new();
        let mut events = Vec::new();
        for chunk in stream.as_bytes().chunks(3) {
            b.push(chunk);
            while let Some(ev) = b.next_event() {
                events.push(ev);
            }
        }
        assert_eq!(events.len(), 3);
        assert!(events[0].contains("سلام دنیا"));
        assert!(events[1].contains("ZWNJ‌تست 🙂"));
        assert_eq!(events[2], "data: [DONE]");
    }

    #[test]
    fn sse_partial_event_and_partial_char_stay_buffered() {
        // "data: " (6 bytes) + س (D8 B3) + ل (D9 84) + ا (D8 A7) + م (D9 85) + \n\n
        let full: &[u8] = b"data: \xd8\xb3\xd9\x84\xd8\xa7\xd9\x85\n\n";
        let mut b = SseBuffer::new();
        b.push(&full[..7]); // "data: " + lone 0xD8 lead byte
        assert_eq!(b.next_event(), None, "incomplete event must stay buffered");
        assert!(!b.is_empty());
        b.push(&full[7..]);
        assert_eq!(b.next_event().as_deref(), Some("data: سلام"));
        assert!(b.is_empty());
        assert_eq!(b.next_event(), None);
    }

    #[test]
    fn sse_multiple_events_in_single_chunk() {
        let mut b = SseBuffer::new();
        b.push(b"data: one\n\ndata: two\n\ndata: three\n\n");
        assert_eq!(b.next_event().as_deref(), Some("data: one"));
        assert_eq!(b.next_event().as_deref(), Some("data: two"));
        assert_eq!(b.next_event().as_deref(), Some("data: three"));
        assert_eq!(b.next_event(), None);
    }

    #[test]
    fn sse_flush_emits_final_event_without_separator() {
        let mut b = SseBuffer::new();
        b.push(b"data: unterminated tail");
        assert_eq!(b.next_event(), None);
        assert_eq!(b.flush().as_deref(), Some("data: unterminated tail"));
        assert_eq!(b.flush(), None);
        assert!(b.is_empty());
    }

    #[test]
    fn sse_separator_only_chunks_produce_nothing() {
        let mut b = SseBuffer::new();
        b.push(b"\n\n");
        assert_eq!(b.next_event().as_deref(), Some("")); // empty event, caller skips
        assert_eq!(b.next_event(), None);
    }

    /// Strict decode must win for valid bytes: the event bytes are
    /// byte-identical to the source (the f844d2df-style guarantee).
    #[test]
    fn sse_decoded_events_are_byte_identical_for_valid_utf8() {
        let payload = "متن فارسی با نیم‌فاصله";
        let stream = sse_frame(&format!("data: {}", payload));
        let mut b = SseBuffer::new();
        b.push(stream.as_bytes());
        let ev = b.next_event().expect("event");
        let expected = format!("data: {}", payload);
        assert_eq!(ev.as_bytes(), expected.as_bytes());
    }
}
