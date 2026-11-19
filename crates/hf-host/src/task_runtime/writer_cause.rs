//! The writer's source tag: one line on stderr before retention, parsed by the owner.
//!
//! Only an exact `harflow-writer-cause: <tag>` line with a known tag is recognised, and only
//! within the first `BOUND` bytes of the writer's stderr. Nothing else of the stream is kept.

const PREFIX: &str = "harflow-writer-cause: ";
/// Where the writer failed; the only values the owner carries into its retained cause.
pub(super) const TAGS: [&str; 5] = [
    "raw_capture_prepare",
    "claim",
    "ready_flush",
    "service",
    "finish",
];
/// The diagnostic stderr bytes inspected in total (the owner-loop stderr read bound).
pub(super) const BOUND: usize = 4096;
const LINE_MAX: usize = 64;

/// Prints the tag line on stderr, best effort.
pub(super) fn emit(tag: &'static str) {
    debug_assert!(TAGS.contains(&tag));
    diag(format!("{PREFIX}{tag}\n").as_bytes());
}
/// One best-effort diagnostic line; it never blocks, retries or panics.
pub(super) fn diag(line: &[u8]) {
    #[cfg(test)]
    if super::seams::diag_hit(line) {
        return;
    }
    hf_platform::task_runtime::diag::diag_line(line);
}
/// Incremental parser fed by every stderr read, including reads whose protocol step failed.
#[derive(Default)]
pub(in crate::task_runtime) struct TagParser {
    seen: usize,
    line: Vec<u8>,
    overlong: bool,
    tag: Option<&'static str>,
}
impl TagParser {
    pub(super) fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if self.seen >= BOUND {
                return;
            }
            self.seen += 1;
            if byte == b'\n' {
                self.finish_line();
            } else if self.line.len() < LINE_MAX {
                self.line.push(byte);
            } else {
                self.overlong = true;
            }
        }
    }
    fn finish_line(&mut self) {
        if self.tag.is_none() && !self.overlong {
            self.tag = std::str::from_utf8(&self.line)
                .ok()
                .and_then(|line| line.strip_prefix(PREFIX))
                .and_then(|tag| TAGS.iter().find(|known| **known == tag).copied());
        }
        self.line.clear();
        self.overlong = false;
    }
    /// The first recognised tag, if its line was complete within the bound.
    pub(super) fn tag(&self) -> Option<&'static str> {
        self.tag
    }
    /// Bytes still inspectable before the bound.
    pub(super) fn remaining(&self) -> usize {
        BOUND - self.seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(chunks: &[&[u8]]) -> Option<&'static str> {
        let mut parser = TagParser::default();
        chunks.iter().for_each(|chunk| parser.feed(chunk));
        parser.tag()
    }

    #[test]
    fn an_exact_line_with_a_known_tag_is_recognised() {
        for tag in TAGS {
            assert_eq!(parsed(&[format!("{PREFIX}{tag}\n").as_bytes()]), Some(tag));
        }
        assert_eq!(
            parsed(&[b"noise\nharflow-writer-cause: claim\nmore\n"]),
            Some("claim")
        );
    }
    #[test]
    fn the_tag_split_across_two_reads_is_reassembled() {
        assert_eq!(
            parsed(&[b"harflow-writer-ca", b"use: ready_fl", b"ush\n"]),
            Some("ready_flush")
        );
        assert_eq!(
            parsed(&[b"harflow-writer-cause: serv", b"ice"]),
            None,
            "no newline yet"
        );
    }
    #[test]
    fn only_exact_lines_count() {
        for line in [
            &b"harflow-writer-cause: unknown\n"[..],
            b"harflow-writer-cause:claim\n",
            b"harflow-writer-cause: claim \n",
            b" harflow-writer-cause: claim\n",
            b"harflow-writer-cause: claim\r\n",
            b"HARFLOW-WRITER-CAUSE: claim\n",
        ] {
            assert_eq!(parsed(&[line]), None, "{line:?}");
        }
        let long = [
            b"x".repeat(200),
            b"\nharflow-writer-cause: claim\n".to_vec(),
        ]
        .concat();
        assert_eq!(
            parsed(&[&long]),
            Some("claim"),
            "an overlong line is skipped, not fatal"
        );
        assert_eq!(
            parsed(&[b"harflow-writer-cause: claim\nharflow-writer-cause: finish\n"]),
            Some("claim")
        );
    }
    #[test]
    fn no_tag_survives_a_bound_overrun() {
        let mut bytes = vec![b'.'; BOUND - 10];
        bytes.extend_from_slice(b"\nharflow-writer-cause: claim\n");
        assert_eq!(parsed(&[&bytes]), None, "the line ends beyond the bound");
        let mut within = vec![b'.'; BOUND - 40];
        within.extend_from_slice(b"\nharflow-writer-cause: claim\n");
        assert_eq!(parsed(&[&within]), Some("claim"));
        let mut parser = TagParser::default();
        parser.feed(&vec![b'.'; BOUND]);
        assert_eq!(parser.remaining(), 0);
    }
}
