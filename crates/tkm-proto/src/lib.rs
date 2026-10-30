#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Protocol types and framing for tokenimum.
//!
//! The hook uses synchronous framing; the daemon can use the pure encoding and
//! decoding helpers with its own transport. Receivers must check message
//! `version` against [`PROTOCOL_VERSION`] before acting on a decoded message.

pub mod monitor;

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::num::TryFromIntError;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;

/// Current wire protocol version; incompatible peers must fail open.
pub const PROTOCOL_VERSION: u16 = 1;

/// Maximum time the hook spends connecting to the daemon.
pub const CONNECT_TIMEOUT: Duration = Duration::from_millis(2);

/// Request JSON body limit (8 MiB), allowing substantial raw tool output.
///
/// The four-byte header is excluded. Hosts may impose smaller input limits.
pub const MAX_REQUEST_FRAME: usize = 8 * 1024 * 1024;

/// Reply JSON body limit (64 KiB), allowing escaped host JSON around capped text.
///
/// The four-byte header is excluded. Adapters still enforce host character or
/// token caps, including Claude's 10,000-character context limit.
pub const MAX_REPLY_FRAME: usize = 64 * 1024;

/// Supported harness identity and stable hook command-line name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Host {
    /// Claude Code.
    Claude,
    /// `OpenAI` Codex.
    Codex,
    /// Antigravity CLI.
    Agy,
    /// Cursor agent and CLI.
    Cursor,
    /// Kimi Code.
    Kimi,
    /// `DeepSeek`'s Codewhale harness.
    Deepseek,
    /// The opencode TypeScript shim.
    Opencode,
    /// The Pi TypeScript shim.
    Pi,
}

impl Host {
    /// Returns the exact name accepted by `tkm-hook --host`.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Agy => "agy",
            Self::Cursor => "cursor",
            Self::Kimi => "kimi",
            Self::Deepseek => "deepseek",
            Self::Opencode => "opencode",
            Self::Pi => "pi",
        }
    }

    /// Returns exact failure stdout, without a trailing newline (SPEC 6.3).
    ///
    /// Empty shim output means return without mutation. Antigravity's `{}`
    /// fallback is inferred in the SPEC and remains subject to host verification.
    #[must_use]
    pub const fn noop_reply(&self) -> &'static str {
        match self {
            Self::Agy | Self::Cursor => "{}",
            Self::Claude
            | Self::Codex
            | Self::Kimi
            | Self::Deepseek
            | Self::Opencode
            | Self::Pi => "",
        }
    }
}

/// A hook command-line host name that is not supported.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown host name: {name}")]
pub struct ParseHostError {
    /// Unrecognized input, retained for diagnostics.
    pub name: String,
}

impl FromStr for Host {
    type Err = ParseHostError;

    /// Parses an exact, lowercase hook command-line name.
    ///
    /// # Errors
    /// Returns [`ParseHostError`] for an unknown name, alias, or different case.
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match name {
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            "agy" => Ok(Self::Agy),
            "cursor" => Ok(Self::Cursor),
            "kimi" => Ok(Self::Kimi),
            "deepseek" => Ok(Self::Deepseek),
            "opencode" => Ok(Self::Opencode),
            "pi" => Ok(Self::Pi),
            _ => Err(ParseHostError { name: name.into() }),
        }
    }
}

/// Normalized event selected by the installer's `--event` argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EventKind {
    /// A new session has started.
    SessionStart,
    /// A user prompt starts or redirects a turn.
    Prompt,
    /// A tool is about to run; Codewhale uses this injection opportunity.
    ToolBefore,
    /// A tool completed or failed.
    ToolDone,
    /// A turn ended.
    TurnEnd,
    /// A session ended.
    SessionEnd,
}

impl EventKind {
    /// Returns the exact name accepted by `tkm-hook --event`.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::SessionStart => "session_start",
            Self::Prompt => "prompt",
            Self::ToolBefore => "tool_before",
            Self::ToolDone => "tool_done",
            Self::TurnEnd => "turn_end",
            Self::SessionEnd => "session_end",
        }
    }

    /// Returns the hook's reply read timeout from SPEC 6.2.
    ///
    /// `tool_before` shares the 5 ms tool-event budget: injection must already
    /// be cached and must never wait for a network decision.
    #[must_use]
    pub const fn read_timeout(&self) -> Duration {
        match self {
            Self::SessionStart => Duration::from_millis(300),
            Self::Prompt => Duration::from_millis(1_500),
            Self::ToolBefore | Self::ToolDone | Self::TurnEnd | Self::SessionEnd => {
                Duration::from_millis(5)
            }
        }
    }
}

/// A hook command-line event name that is not supported.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown event kind: {name}")]
pub struct ParseEventKindError {
    /// Unrecognized input, retained for diagnostics.
    pub name: String,
}

impl FromStr for EventKind {
    type Err = ParseEventKindError;

    /// Parses an exact, lowercase hook command-line event name.
    ///
    /// # Errors
    /// Returns [`ParseEventKindError`] for unknown names or different casing.
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match name {
            "session_start" => Ok(Self::SessionStart),
            "prompt" => Ok(Self::Prompt),
            "tool_before" => Ok(Self::ToolBefore),
            "tool_done" => Ok(Self::ToolDone),
            "turn_end" => Ok(Self::TurnEnd),
            "session_end" => Ok(Self::SessionEnd),
            _ => Err(ParseEventKindError { name: name.into() }),
        }
    }
}

/// Raw hook input forwarded without host interpretation by the hook binary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    /// Wire protocol version; receivers must compare with [`PROTOCOL_VERSION`].
    pub version: u16,
    /// Host selected by the installer.
    pub host: Host,
    /// Normalized event used to select a timeout before reading a reply.
    pub event: EventKind,
    /// Native host event name, if known.
    pub host_event: Option<String>,
    /// Raw stdin JSON, or [`serde_json::Value::Null`] for no stdin.
    pub payload: serde_json::Value,
    /// Adapter-selected environment markers; never the whole process environment.
    pub env: BTreeMap<String, String>,
    /// Working directory, if available, as a UTF-8 host-supplied path.
    pub cwd: Option<String>,
}

/// Semantic action for daemon logging and contract tests.
///
/// The hook never interprets this field; it prints [`Reply::stdout`] verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReplyAction {
    /// Leave the host session unchanged.
    #[default]
    Nothing,
    /// Add context through the host's supported injection surface.
    Inject {
        /// Context text before host rendering and host-specific capping.
        text: String,
    },
    /// Show an advisory notice where the host supports it.
    Notice {
        /// Text intended for the person.
        text: String,
    },
    /// Request both context injection and an advisory notice.
    InjectAndNotice {
        /// Context text before host rendering and host-specific capping.
        inject: String,
        /// Text intended for the person.
        notice: String,
    },
}

/// Daemon reply containing the adapter's fully rendered host output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    /// Wire protocol version; receivers must compare with [`PROTOCOL_VERSION`].
    pub version: u16,
    /// Semantic action for logging and tests; never interpreted by the hook.
    pub action: ReplyAction,
    /// Exact stdout to print, without adding a newline or re-serializing JSON.
    pub stdout: String,
    /// Whether session context or steering remains queued; absent means unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<bool>,
}

/// Failure to encode, transport, or decode one length-prefixed JSON frame.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum FrameError {
    /// JSON body length exceeds the caller's limit.
    #[error("frame body length {len} exceeds limit {max}")]
    Oversize {
        /// Advertised or serialized JSON body length in bytes.
        len: usize,
        /// Allowed JSON body length in bytes.
        max: usize,
    },
    /// EOF occurred before all four header bytes or the complete body arrived.
    #[error("truncated frame header or body")]
    Truncated,
    /// A zero-length header or empty body cannot contain a JSON value.
    #[error("empty frame body")]
    Empty,
    /// JSON serialization, UTF-8 decoding, syntax, or target shape failed.
    #[error("malformed frame JSON: {0}")]
    Malformed(
        /// Original serialization or deserialization failure.
        #[from]
        serde_json::Error,
    ),
    /// Body length cannot be represented by the wire or native integer type.
    #[error("frame length cannot be represented: {0}")]
    LengthOverflow(
        /// Original checked integer conversion failure.
        #[from]
        TryFromIntError,
    ),
    /// Transport failure other than an incomplete read.
    #[error("frame I/O failed: {0}")]
    Io(
        /// Original transport failure, retaining its error kind.
        #[from]
        io::Error,
    ),
}

/// Encodes a four-byte big-endian body length followed by UTF-8 JSON.
///
/// `max` counts only body bytes. Serialization completes before any transport
/// write, so serialization or size failures never emit a partial frame.
///
/// # Errors
/// Returns [`FrameError::Malformed`] if serialization fails,
/// [`FrameError::Oversize`] above `max`, or [`FrameError::LengthOverflow`]
/// when the body does not fit the wire's `u32` length.
pub fn encode_frame<T: Serialize + ?Sized>(value: &T, max: usize) -> Result<Vec<u8>, FrameError> {
    let body = serde_json::to_vec(value)?;
    if body.len() > max {
        return Err(FrameError::Oversize {
            len: body.len(),
            max,
        });
    }
    let length = u32::try_from(body.len())?;
    let mut frame = Vec::new();
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}

/// Validates a big-endian length header before a transport allocates the body.
///
/// # Errors
/// Returns [`FrameError::Empty`] for zero, [`FrameError::Oversize`] above
/// `max`, or [`FrameError::LengthOverflow`] if the native size cannot hold it.
pub fn decode_len(header: [u8; 4], max: usize) -> Result<usize, FrameError> {
    let len = usize::try_from(u32::from_be_bytes(header))?;
    if len == 0 {
        return Err(FrameError::Empty);
    }
    if len > max {
        return Err(FrameError::Oversize { len, max });
    }
    Ok(len)
}

/// Decodes exactly one JSON body, rejecting trailing non-whitespace data.
///
/// Transports must call [`decode_len`] before allocating or reading this body.
/// This helper does not validate protocol versions; callers must do so.
///
/// # Errors
/// Returns [`FrameError::Empty`] for an empty slice and [`FrameError::Malformed`]
/// for invalid UTF-8, invalid JSON, or a value incompatible with `T`.
pub fn decode_body<T: DeserializeOwned>(body: &[u8]) -> Result<T, FrameError> {
    if body.is_empty() {
        return Err(FrameError::Empty);
    }
    Ok(serde_json::from_slice(body)?)
}

/// Writes one encoded frame synchronously, without flushing the writer.
///
/// # Errors
/// Returns the serialization or size errors from [`encode_frame`], or
/// [`FrameError::Io`] if the writer fails. A transport failure can leave a
/// partial frame on the writer; callers must discard that connection.
pub fn write_frame<W: Write, T: Serialize + ?Sized>(
    mut writer: W,
    value: &T,
    max: usize,
) -> Result<(), FrameError> {
    let frame = encode_frame(value, max)?;
    writer.write_all(&frame)?;
    Ok(())
}

/// Reads one frame synchronously, leaving any following frame unread.
///
/// The header is validated before allocating or reading body bytes.
///
/// # Errors
/// Returns [`FrameError::Truncated`] on any premature EOF, including an empty
/// stream; other I/O errors retain their source as [`FrameError::Io`]. Also
/// returns header and JSON errors from [`decode_len`] and [`decode_body`].
/// Callers must discard the connection after an error.
///
/// # Examples
/// ```
/// use std::io::Cursor;
/// use tkm_proto::{encode_frame, read_frame, MAX_REPLY_FRAME};
/// let frame = encode_frame("context", MAX_REPLY_FRAME)?;
/// let text: String = read_frame(Cursor::new(frame), MAX_REPLY_FRAME)?;
/// assert_eq!(text, "context");
/// # Ok::<(), tkm_proto::FrameError>(())
/// ```
pub fn read_frame<R: Read, T: DeserializeOwned>(
    mut reader: R,
    max: usize,
) -> Result<T, FrameError> {
    let mut header = [0; 4];
    reader.read_exact(&mut header).map_err(read_error)?;
    let len = decode_len(header, max)?;
    let mut body = vec![0; len];
    reader.read_exact(&mut body).map_err(read_error)?;
    decode_body(&body)
}

fn read_error(error: io::Error) -> FrameError {
    if error.kind() == io::ErrorKind::UnexpectedEof {
        FrameError::Truncated
    } else {
        FrameError::Io(error)
    }
}
