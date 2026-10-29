//! JSON token spans; serde validates grammar before this scanner sees input.

use crate::AdapterError;
use serde_json::Value;
use std::ops::Range;

#[derive(Debug)]
pub(super) struct Member {
    pub key: String,
    pub start: usize,
    pub value: Node,
}

#[derive(Debug)]
pub(super) struct Node {
    pub range: Range<usize>,
    pub members: Vec<Member>,
    pub elements: Vec<Self>,
    pub object: bool,
    pub array: bool,
}

impl Node {
    pub fn member(&self, name: &str) -> Option<&Self> {
        self.members
            .iter()
            .rev()
            .find(|member| member.key == name)
            .map(|member| &member.value)
    }
}

pub(super) fn parse(text: &str) -> Result<Node, AdapterError> {
    let _: Value = serde_json::from_str(text)?;
    let mut scanner = Scanner {
        bytes: text.as_bytes(),
        cursor: 0,
    };
    scanner.value()
}

struct Scanner<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl Scanner<'_> {
    fn whitespace(&mut self) {
        while self
            .bytes
            .get(self.cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.cursor += 1;
        }
    }

    fn string_end(&mut self) {
        self.cursor += 1;
        while let Some(byte) = self.bytes.get(self.cursor) {
            self.cursor += 1;
            match byte {
                b'\\' => self.cursor += 1,
                b'"' => break,
                _ => {}
            }
        }
    }

    fn value(&mut self) -> Result<Node, AdapterError> {
        self.whitespace();
        let start = self.cursor;
        let mut node = Node {
            range: start..start,
            members: Vec::new(),
            elements: Vec::new(),
            object: false,
            array: false,
        };
        match self.bytes.get(self.cursor) {
            Some(b'{') => {
                node.object = true;
                self.cursor += 1;
                loop {
                    self.whitespace();
                    if self.bytes.get(self.cursor) == Some(&b'}') {
                        self.cursor += 1;
                        break;
                    }
                    let key_start = self.cursor;
                    self.string_end();
                    let key: String = serde_json::from_slice(&self.bytes[key_start..self.cursor])?;
                    self.whitespace();
                    self.cursor += 1; // validated colon
                    let value = self.value()?;
                    node.members.push(Member {
                        key,
                        start: key_start,
                        value,
                    });
                    self.whitespace();
                    if self.bytes.get(self.cursor) == Some(&b',') {
                        self.cursor += 1;
                    }
                }
            }
            Some(b'[') => {
                node.array = true;
                self.cursor += 1;
                loop {
                    self.whitespace();
                    if self.bytes.get(self.cursor) == Some(&b']') {
                        self.cursor += 1;
                        break;
                    }
                    node.elements.push(self.value()?);
                    self.whitespace();
                    if self.bytes.get(self.cursor) == Some(&b',') {
                        self.cursor += 1;
                    }
                }
            }
            Some(b'"') => self.string_end(),
            _ => {
                while self.bytes.get(self.cursor).is_some_and(|byte| {
                    !byte.is_ascii_whitespace() && !matches!(byte, b',' | b']' | b'}')
                }) {
                    self.cursor += 1;
                }
            }
        }
        node.range.end = self.cursor;
        Ok(node)
    }
}

fn prefix_at(text: &str, position: usize) -> &str {
    let start = text[..position].rfind('\n').map_or(0, |index| index + 1);
    let prefix = &text[start..position];
    if prefix.bytes().all(|byte| matches!(byte, b' ' | b'\t')) {
        prefix
    } else {
        ""
    }
}

fn insertion(text: &str, node: &Node, last: Option<(usize, usize)>, content: &str) -> String {
    let position = last.map_or(node.range.start + 1, |(_, end)| end);
    let comma = if last.is_some() { "," } else { "" };
    let fragment = if text[node.range.clone()].contains('\n') {
        let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let indent = last
            .map(|(start, _)| prefix_at(text, start))
            .filter(|indent| !indent.is_empty())
            .map_or_else(
                || {
                    format!(
                        "{}{}",
                        prefix_at(text, node.range.start),
                        if text.contains('\t') { "\t" } else { "  " }
                    )
                },
                str::to_owned,
            );
        format!("{comma}{newline}{indent}{content}")
    } else {
        format!("{comma}{content}")
    };
    let mut changed = text.to_owned();
    changed.insert_str(position, &fragment);
    changed
}

pub(super) fn append_member(text: &str, node: &Node, key: &str, value: &Value) -> String {
    let last = node
        .members
        .last()
        .map(|member| (member.start, member.value.range.end));
    insertion(
        text,
        node,
        last,
        &format!("{}:{value}", Value::String(key.to_owned())),
    )
}

pub(super) fn replace_value(text: &str, node: &Node, value: &Value) -> String {
    let mut changed = text.to_owned();
    changed.replace_range(node.range.clone(), &value.to_string());
    changed
}

pub(super) fn append_element(text: &str, node: &Node, value: &Value) -> String {
    let last = node
        .elements
        .last()
        .map(|value| (value.range.start, value.range.end));
    insertion(text, node, last, &value.to_string())
}

fn remove_item(
    text: &str,
    container_start: usize,
    spans: &[(usize, usize)],
    index: usize,
) -> String {
    let (start, end) = spans[index];
    let range = if index > 0 {
        spans[index - 1].1..end
    } else if let Some((next, _)) = spans.get(1) {
        let comma = text[end..*next]
            .find(',')
            .map_or(end, |offset| end + offset + 1);
        start..comma
    } else {
        container_start + 1..end
    };
    let mut changed = text.to_owned();
    changed.replace_range(range, "");
    changed
}

pub(super) fn remove_element(text: &str, node: &Node, index: usize) -> String {
    let spans: Vec<_> = node
        .elements
        .iter()
        .map(|value| (value.range.start, value.range.end))
        .collect();
    remove_item(text, node.range.start, &spans, index)
}

pub(super) fn remove_member(text: &str, node: &Node, key: &str) -> String {
    let Some(index) = node.members.iter().rposition(|member| member.key == key) else {
        return text.to_owned();
    };
    let spans: Vec<_> = node
        .members
        .iter()
        .map(|member| (member.start, member.value.range.end))
        .collect();
    remove_item(text, node.range.start, &spans, index)
}
