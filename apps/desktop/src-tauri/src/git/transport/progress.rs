use std::fmt::{Debug, Formatter};

use super::model::{NetworkByteProgress, NetworkObjectProgress, NetworkStage};

const MAX_INCOMPLETE_LINE_BYTES: usize = 8 * 1024;

#[derive(Clone, PartialEq)]
pub struct GitProgressEvent {
    pub stage: NetworkStage,
    pub fraction: Option<f64>,
    pub objects: Option<NetworkObjectProgress>,
    pub bytes: Option<NetworkByteProgress>,
}

impl Debug for GitProgressEvent {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GitProgressEvent")
            .field("stage", &self.stage)
            .field("fraction", &self.fraction)
            .field("objects", &self.objects)
            .field("bytes", &self.bytes)
            .finish()
    }
}

#[derive(Default)]
pub struct GitProgressParser {
    pending: Vec<u8>,
    dropping_oversized_line: bool,
}

impl Debug for GitProgressParser {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GitProgressParser")
            .field("buffered_bytes", &self.pending.len())
            .field("dropping_oversized_line", &self.dropping_oversized_line)
            .finish()
    }
}

impl GitProgressParser {
    pub fn push(&mut self, chunk: &[u8]) -> Vec<GitProgressEvent> {
        let mut events = Vec::new();
        for byte in chunk {
            if matches!(byte, b'\r' | b'\n') {
                if !self.dropping_oversized_line
                    && !self.pending.is_empty()
                    && let Some(event) = parse_progress_line(&self.pending)
                {
                    events.push(event);
                }
                self.pending.clear();
                self.dropping_oversized_line = false;
                continue;
            }
            if self.dropping_oversized_line {
                continue;
            }
            if self.pending.len() == MAX_INCOMPLETE_LINE_BYTES {
                self.pending.clear();
                self.dropping_oversized_line = true;
                continue;
            }
            self.pending.push(*byte);
        }
        events
    }

    pub fn buffered_len(&self) -> usize {
        self.pending.len()
    }
}

fn parse_progress_line(bytes: &[u8]) -> Option<GitProgressEvent> {
    let line = std::str::from_utf8(bytes).ok()?.trim();
    let line = line.strip_prefix("remote: ").unwrap_or(line);
    const PREFIXES: &[&str] = &[
        "Enumerating objects:",
        "Counting objects:",
        "Compressing objects:",
        "Receiving objects:",
        "Resolving deltas:",
        "Writing objects:",
    ];
    let progress = PREFIXES
        .iter()
        .find_map(|prefix| line.strip_prefix(prefix))?
        .trim();
    let percent_end = progress.find('%')?;
    let percent_start = progress[..percent_end]
        .rfind(|character: char| !character.is_ascii_digit())
        .map_or(0, |index| index + 1);
    let percent = progress[percent_start..percent_end].parse::<u8>().ok()?;
    if percent > 100 {
        return None;
    }
    let objects = parse_object_count(&progress[percent_end + 1..]);
    let bytes = parse_byte_count(progress);
    Some(GitProgressEvent {
        stage: NetworkStage::Transferring,
        fraction: Some(f64::from(percent) / 100.0),
        objects,
        bytes,
    })
}

fn parse_object_count(value: &str) -> Option<NetworkObjectProgress> {
    let start = value.find('(')? + 1;
    let end = value[start..].find(')')? + start;
    let (completed, total) = value[start..end].split_once('/')?;
    Some(NetworkObjectProgress {
        completed: completed.trim().parse().ok()?,
        total: Some(total.trim().parse().ok()?),
    })
}

fn parse_byte_count(value: &str) -> Option<NetworkByteProgress> {
    let object_end = value.find(')')?;
    let before_rate = value[object_end + 1..].split('|').next()?.trim();
    let quantity = before_rate.strip_prefix(',')?.trim();
    let mut fields = quantity.split_whitespace();
    let amount = fields.next()?.parse::<f64>().ok()?;
    let multiplier = match fields.next()? {
        "B" => 1_f64,
        "KiB" => 1024_f64,
        "MiB" => 1024_f64 * 1024_f64,
        "GiB" => 1024_f64 * 1024_f64 * 1024_f64,
        _ => return None,
    };
    let transferred = amount * multiplier;
    if !transferred.is_finite() || transferred.is_sign_negative() || transferred > u64::MAX as f64 {
        return None;
    }
    Some(NetworkByteProgress {
        transferred: transferred.round() as u64,
        total: None,
    })
}

#[cfg(test)]
mod tests {
    use super::GitProgressParser;
    use crate::git::transport::model::NetworkStage;

    #[test]
    fn parses_receiving_and_writing_progress_without_retaining_raw_remote_text() {
        let mut parser = GitProgressParser::default();
        let events = parser
            .push(b"remote: secret text\nReceiving objects: 42% (42/100), 1.00 MiB | 2.00 MiB/s\r");
        assert!(events.iter().any(|event| {
            event.stage == NetworkStage::Transferring && event.fraction == Some(0.42)
        }));
        assert!(
            events
                .iter()
                .all(|event| !format!("{event:?}").contains("secret text"))
        );
        let remote_events = parser.push(b"remote: Counting objects: 10% (1/10)\r");
        assert_eq!(remote_events.len(), 1);
        assert_eq!(remote_events[0].fraction, Some(0.1));
    }

    #[test]
    fn parser_handles_split_lines_and_bounds_unrecognized_input() {
        let mut parser = GitProgressParser::default();
        assert!(parser.push(b"Writing objects: 5").is_empty());
        let events = parser.push(b"0% (5/10)\r");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].fraction, Some(0.5));
        assert_eq!(events[0].objects.as_ref().unwrap().completed, 5);
        assert_eq!(events[0].objects.as_ref().unwrap().total, Some(10));

        let private_line = vec![b'x'; 32 * 1024];
        assert!(parser.push(&private_line).is_empty());
        assert!(parser.buffered_len() <= 8 * 1024);
        assert!(!format!("{parser:?}").contains(&"x".repeat(32)));
    }
}
