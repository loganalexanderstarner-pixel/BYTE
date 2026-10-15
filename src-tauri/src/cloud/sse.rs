//! Incremental Server-Sent Events parser: bytes arrive in arbitrary chunks
//! (a line or an event can be split anywhere), events come out whole.

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub event: String,
    pub data: String,
    pub id: Option<String>,
}

#[derive(Default)]
pub struct Parser {
    buf: String,
    event: String,
    data: Vec<String>,
    id: Option<String>,
}

impl Parser {
    /// Feeds a chunk; returns the events it completed.
    pub fn push(&mut self, chunk: &str) -> Vec<Event> {
        self.buf.push_str(chunk);
        let mut out = Vec::new();
        while let Some(nl) = self.buf.find('\n') {
            let line: String = self.buf.drain(..=nl).collect();
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                if !self.data.is_empty() || !self.event.is_empty() {
                    out.push(Event {
                        event: if self.event.is_empty() { "message".into() } else { std::mem::take(&mut self.event) },
                        data: self.data.join("\n"),
                        id: self.id.clone(),
                    });
                }
                self.event.clear();
                self.data.clear();
                continue;
            }
            if line.starts_with(':') {
                continue; // keep-alive comment
            }
            let (field, value) = match line.split_once(':') {
                Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
                None => (line, ""),
            };
            match field {
                "event" => self.event = value.to_string(),
                "data" => self.data.push(value.to_string()),
                "id" => self.id = Some(value.to_string()),
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_survive_arbitrary_splits() {
        let stream = ": ping\n\nevent: delta\ndata: {\"id\":7,\"append\":\"Hel\"}\n\nevent: delta\r\ndata: {\"id\":7,\"append\":\"lo\"}\r\n\r\nevent: done\ndata: {}\n\n";
        for size in [1, 2, 3, 7, 13, stream.len()] {
            let mut p = Parser::default();
            let mut got = Vec::new();
            let bytes: Vec<char> = stream.chars().collect();
            for c in bytes.chunks(size) {
                got.extend(p.push(&c.iter().collect::<String>()));
            }
            assert_eq!(got.len(), 3, "chunk size {size}");
            assert_eq!(got[0].event, "delta");
            assert_eq!(got[1].data, "{\"id\":7,\"append\":\"lo\"}");
            assert_eq!(got[2].event, "done");
        }
    }

    #[test]
    fn multi_line_data_and_default_event() {
        let mut p = Parser::default();
        let e = p.push("data: a\ndata: b\n\n");
        assert_eq!(e, vec![Event { event: "message".into(), data: "a\nb".into(), id: None }]);
    }
}
