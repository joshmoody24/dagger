//! Just enough of the language server protocol to ask a language server questions.
//!
//! Hand-rolled rather than pulled in, because only a handful of messages are needed and
//! the shapes are stable. Framing is a Content-Length header, then a JSON body.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
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
                        // Unasked, symbols come back as a flat list of names and whole
                        // ranges. The nested form also says where each name sits, which
                        // is the spot everything else gets asked about.
                        "documentSymbol": { "hierarchicalDocumentSymbolSupport": true },
                    },
                },
                "initializationOptions": options,
            }),
        )?;
        self.notify("initialized", json!({}))
    }

    /// Reads notifications until one satisfies `settled`. Some servers answer questions
    /// before they've finished indexing, and those answers are wrong rather than slow,
    /// which is worse.
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

    /// Reads the next message meant for us, answering anything the server asks of the
    /// client along the way.
    ///
    /// Conversation runs both ways: a server may ask the client to register a capability
    /// or hand over configuration, and it waits for the answer before doing anything
    /// else. Ignoring those questions doesn't lose a feature, it wedges the server.
    fn read_message(&mut self) -> Result<Value> {
        loop {
            let message = self.read_raw()?;
            let asking = message.get("id").is_some() && message.get("method").is_some();
            if !asking {
                return Ok(message);
            }

            let result = match message["method"].as_str() {
                // One answer per thing asked about, and none of them a setting we hold
                // an opinion on.
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

/// Byte offsets from the parser, line and UTF-16 column for the protocol.
pub struct Lines {
    starts: Vec<usize>,
    text: String,
}

impl Lines {
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            text.char_indices()
                .filter(|(_, character)| *character == '\n')
                .map(|(at, _)| at + 1),
        );
        Self {
            starts,
            text: text.to_string(),
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

    pub fn slice(&self, range: &std::ops::Range<usize>) -> &str {
        &self.text[range.clone()]
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
