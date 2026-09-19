//! Just enough of the language server protocol to ask rust-analyzer questions.
//!
//! Hand-rolled rather than pulled in, because only four messages are needed and the
//! shapes are stable. Framing is a Content-Length header, then a JSON body.

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
    /// Starts the server and waits until it has finished thinking. Answers given before
    /// then are wrong rather than slow, which is worse.
    pub fn start(root: &Path) -> Result<Self> {
        let mut child = Command::new("rust-analyzer")
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("couldn't start rust-analyzer; is it on PATH?")?;

        let to_server = child.stdin.take().expect("stdin was piped");
        let from_server = BufReader::new(child.stdout.take().expect("stdout was piped"));
        let mut server = Self {
            child,
            to_server,
            from_server,
            next_id: 1,
        };

        server.handshake(root)?;
        Ok(server)
    }

    fn handshake(&mut self, root: &Path) -> Result<()> {
        let root = root.canonicalize()?;
        self.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": uri(&root),
                "capabilities": {
                    "window": { "workDoneProgress": true },
                    "experimental": { "serverStatusNotification": true },
                    // Without asking for markdown, hover arrives as prose with the
                    // signature buried in it rather than fenced off.
                    "textDocument": {
                        "hover": { "contentFormat": ["markdown"] },
                    },
                },
                // Nothing here needs macros expanded or build scripts run, and both cost
                // real time on a cold tree.
                "initializationOptions": {
                    "cargo": { "buildScripts": { "enable": false } },
                    "procMacro": { "enable": false },
                },
            }),
        )?;
        self.notify("initialized", json!({}))?;
        self.wait_until_ready()
    }

    /// rust-analyzer says when it has stopped indexing. Without waiting, a reference
    /// query answers from a half-built picture and quietly reports too little.
    fn wait_until_ready(&mut self) -> Result<()> {
        loop {
            let message = self.read_message()?;
            if message["method"] == "experimental/serverStatus"
                && message["params"]["quiescent"] == Value::Bool(true)
            {
                return Ok(());
            }
        }
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

    fn read_message(&mut self) -> Result<Value> {
        let mut length = None;
        loop {
            let mut line = String::new();
            if self.from_server.read_line(&mut line)? == 0 {
                bail!("rust-analyzer stopped talking");
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
