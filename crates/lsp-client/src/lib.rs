//! Just enough of the language server protocol to ask a language server questions.
//!
//! Hand-rolled rather than pulled in, because only a handful of messages are needed and
//! the shapes are stable. Framing is a Content-Length header, then a JSON body.

pub mod frontier;
pub mod walk;

use anyhow::{Context, Result, bail};
use dagger_core::model::{Piece, Span};
use dagger_core::prose::line_starts;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::ops::Range;
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub struct Server {
    child: Child,
    to_server: ChildStdin,
    from_server: BufReader<ChildStdout>,
    next_id: i64,
}

impl Server {
    /// Starts a server and shakes hands with it. `options` becomes the server's
    /// initializationOptions, which is where a particular server's own knobs live.
    pub fn start(command: &[String], root: &Path, options: Value) -> Result<Self> {
        let (program, args) = command
            .split_first()
            .context("no language server was named")?;

        let mut child = Command::new(program)
            .args(args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("couldn't start {program}; is it on PATH?"))?;

        let to_server = child.stdin.take().expect("stdin was piped");
        let from_server = BufReader::new(child.stdout.take().expect("stdout was piped"));
        let mut server = Self {
            child,
            to_server,
            from_server,
            next_id: 1,
        };

        server.handshake(root, options)?;
        Ok(server)
    }

    fn handshake(&mut self, root: &Path, options: Value) -> Result<()> {
        let root = root.canonicalize()?;
        self.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": uri(&root),
                "capabilities": {
                    "window": { "workDoneProgress": true },
                    // rust-analyzer offers this and says when it has stopped indexing.
                    // Servers that don't understand it ignore it.
                    "experimental": { "serverStatusNotification": true },
                    "textDocument": {
                        // Without asking for markdown, hover arrives as prose with the
                        // signature buried in it rather than fenced off.
                        "hover": { "contentFormat": ["markdown"] },
                        // The nested form also says where each name sits, which is where
                        // everything else gets asked about.
                        "documentSymbol": { "hierarchicalDocumentSymbolSupport": true },
                    },
                },
                "initializationOptions": options,
            }),
        )?;
        self.notify("initialized", json!({}))
    }

    /// Reads notifications until one satisfies `settled`. Some servers answer before they've
    /// finished indexing, and those answers are wrong rather than slow.
    pub fn wait_until(&mut self, settled: impl Fn(&Value) -> bool) -> Result<()> {
        loop {
            let message = self.read_message()?;
            if settled(&message) {
                return Ok(());
            }
        }
    }

    /// Telling a server about a file is what makes it load the project that file belongs
    /// to. Servers that index everything up front don't mind hearing it anyway.
    pub fn open(&mut self, path: &Path, language: &str, text: &str) -> Result<()> {
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri(path),
                    "languageId": language,
                    "version": 1,
                    "text": text,
                },
            }),
        )
    }

    pub fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;

        loop {
            let message = self.read_message()?;
            if message["id"] == json!(id) {
                if let Some(error) = message.get("error") {
                    bail!("{method} failed: {error}");
                }
                return Ok(message["result"].clone());
            }
        }
    }

    pub fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    fn send(&mut self, message: Value) -> Result<()> {
        let body = serde_json::to_vec(&message)?;
        write!(self.to_server, "Content-Length: {}\r\n\r\n", body.len())?;
        self.to_server.write_all(&body)?;
        self.to_server.flush()?;
        Ok(())
    }

    /// Reads the next message meant for us, answering the server's own requests along the
    /// way. A server waits for those answers before doing anything else, so ignoring them
    /// wedges it.
    fn read_message(&mut self) -> Result<Value> {
        loop {
            let message = self.read_raw()?;
            let asking = message.get("id").is_some() && message.get("method").is_some();
            if !asking {
                return Ok(message);
            }

            let result = match message["method"].as_str() {
                // One answer per item, none of them a setting we have an opinion on.
                Some("workspace/configuration") => Value::Array(
                    message["params"]["items"]
                        .as_array()
                        .map(|items| vec![Value::Null; items.len()])
                        .unwrap_or_default(),
                ),
                _ => Value::Null,
            };
            self.send(json!({"jsonrpc": "2.0", "id": message["id"], "result": result}))?;
        }
    }

    fn read_raw(&mut self) -> Result<Value> {
        let mut length = None;
        loop {
            let mut line = String::new();
            if self.from_server.read_line(&mut line)? == 0 {
                bail!("the language server stopped talking");
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length: ") {
                length = Some(value.parse::<usize>()?);
            }
        }

        let length = length.context("a message arrived with no length")?;
        let mut body = vec![0; length];
        self.from_server.read_exact(&mut body)?;
        Ok(serde_json::from_slice(&body)?)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.notify("exit", json!({}));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn uri(path: &Path) -> String {
    format!("file://{}", path.display())
}

/// The declaration out of a hover's markdown: the last fenced block before the `---` rule.
/// Past the rule is documentation, whose code examples are fenced too, and editing an
/// example must not read as breaking every caller. `language` opens only fences marked
/// with it, for servers that fence the module's name too.
pub fn fenced(hover: &Value, language: Option<&str>) -> Option<String> {
    let markdown = hover["contents"]["value"].as_str()?;
    let declaration = markdown.split("\n---").next().unwrap_or(markdown);
    let mut blocks = Vec::new();
    let mut current: Option<Vec<&str>> = None;

    for line in declaration.lines() {
        match (&mut current, line.strip_prefix("```")) {
            (None, Some(fence)) if language.is_none_or(|wanted| fence.starts_with(wanted)) => {
                current = Some(Vec::new())
            }
            (Some(code), Some(_)) => {
                blocks.push(code.join("\n"));
                current = None;
            }
            (Some(code), None) => code.push(line),
            _ => {}
        }
    }

    blocks.into_iter().rfind(|block| !block.is_empty())
}

/// Byte offsets from the parser, line and UTF-16 column for the protocol.
pub struct Lines {
    starts: Vec<usize>,
    text: String,
}

impl Lines {
    pub fn new(text: &str) -> Self {
        Self {
            starts: line_starts(text),
            text: text.to_string(),
        }
    }

    /// One stretch of the file as a definition's part, with the line it starts on.
    pub fn piece(&self, file: &str, range: &Range<usize>) -> Piece {
        Piece {
            text: self.slice(range).to_string(),
            span: Span {
                start: range.start as u32,
                end: range.end as u32,
            },
            line: self.position(range.start).0 + 1,
            file: file.to_string(),
        }
    }

    pub fn position(&self, offset: usize) -> (u32, u32) {
        let line = self.starts.partition_point(|start| *start <= offset) - 1;
        let start = self.starts[line];
        let column = self.text[start..offset.min(self.text.len())]
            .chars()
            .map(|character| character.len_utf16())
            .sum::<usize>();
        (line as u32, column as u32)
    }

    pub fn slice(&self, range: &Range<usize>) -> &str {
        &self.text[range.clone()]
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn offset(&self, line: u32, column: u32) -> usize {
        let start = match self.starts.get(line as usize) {
            Some(start) => *start,
            None => return self.text.len(),
        };
        let mut offset = start;
        let mut remaining = column as usize;
        for character in self.text[start..].chars() {
            if remaining == 0 {
                break;
            }
            remaining = remaining.saturating_sub(character.len_utf16());
            offset += character.len_utf8();
        }
        offset
    }
}

#[cfg(test)]
mod tests {
    use super::{Lines, fenced};
    use serde_json::json;

    const SOURCE: &str = "export function zero() {\n  return 0;\n}\n";

    #[test]
    fn a_type_stops_at_the_documentation() {
        let hover = json!({
            "contents": { "value": "```ts\nfunction add(a: number): number\n```\n---\nAdds.\n\n```ts\nadd(1)\n```" }
        });

        assert_eq!(
            fenced(&hover, None).as_deref(),
            Some("function add(a: number): number")
        );
    }

    /* Servers often put the definition's module in a block of its own first. */
    #[test]
    fn the_last_block_before_the_rule_is_the_declaration() {
        let hover = json!({
            "contents": { "value": "```ts\nmodule \"money\"\n```\n```ts\nconst pence: number\n```" }
        });

        assert_eq!(fenced(&hover, None).as_deref(), Some("const pence: number"));
    }

    #[test]
    fn a_hover_with_nothing_fenced_says_nothing() {
        assert_eq!(
            fenced(&json!({ "contents": { "value": "Adds." } }), None),
            None
        );
    }

    #[test]
    fn only_fences_in_the_language_asked_for_open_a_block() {
        let hover = json!({
            "contents": { "value": "```text\nnot this\n```\n```rust\nfn add(a: u8) -> u8\n```" }
        });

        assert_eq!(
            fenced(&hover, Some("rust")).as_deref(),
            Some("fn add(a: u8) -> u8")
        );
    }

    #[test]
    fn a_position_and_an_offset_mean_the_same_spot() {
        let lines = Lines::new(SOURCE);

        for (line, column) in [(0, 0), (0, 16), (1, 2), (2, 0)] {
            let offset = lines.offset(line, column);
            assert_eq!(
                lines.position(offset),
                (line, column),
                "{line}:{column} didn't survive the round trip"
            );
        }
    }

    #[test]
    fn an_offset_lands_where_the_name_starts() {
        let lines = Lines::new(SOURCE);
        let at = lines.offset(0, 16);

        assert!(SOURCE[at..].starts_with("zero"));
    }

    /// Columns in the protocol count UTF-16 units, so anything outside the basic plane
    /// counts twice. Getting this wrong shifts every position after it on the line.
    #[test]
    fn columns_count_the_way_the_protocol_counts() {
        let source = "const 🎉 = \"party\";\nconst after = 1;\n";
        let lines = Lines::new(source);

        // The emoji is two UTF-16 units, so `=` sits at column 9 rather than 8.
        let equals = lines.offset(0, 9);
        assert_eq!(&source[equals..equals + 1], "=");
        assert_eq!(lines.position(equals), (0, 9));
    }

    #[test]
    fn a_line_past_the_end_lands_at_the_end() {
        let lines = Lines::new(SOURCE);

        assert_eq!(lines.offset(99, 0), SOURCE.len());
    }
}
