// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/loopguard/shell_edits.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Heuristic shell-write detection; no shell execution or filesystem access.
/// Ordered unique basenames written by a shell command, as in TE.
/// Unresolved script and patch targets use `(script)` and `(patch)`.
#[must_use]
pub fn shell_edit_targets(command: &str) -> Vec<String> {
    let segments = shell_segments(command);
    let writes = segments
        .iter()
        .flat_map(|segment| segment.writes.iter())
        .filter(|path| source_file(path))
        .cloned();
    let edits = segments.iter().flat_map(command_targets);
    normalize_targets(writes.chain(edits))
}

fn normalize_targets(paths: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut names = Vec::new();
    for path in paths {
        if path.is_empty()
            || ["/tmp/", "/dev/", "/proc/"]
                .iter()
                .any(|p| path.starts_with(p))
        {
            continue;
        }
        let name = if path.starts_with('(') {
            path.as_str()
        } else {
            basename(&path)
        };
        if !name.is_empty() && !names.iter().any(|s| s == name) {
            names.push(name.to_owned());
        }
    }
    names
}

/// One executable shell stage, with only its own stdin bodies and redirects.
#[derive(Default)]
pub(super) struct ShellSegment {
    pub(super) words: Vec<String>,
    pub(super) bodies: Vec<String>,
    pub(super) writes: Vec<String>,
    pub(super) next: String,
}

pub(super) fn shell_segments(command: &str) -> Vec<ShellSegment> {
    walk(command, 0)
}

fn walk(command: &str, depth: u8) -> Vec<ShellSegment> {
    if depth >= 16 {
        return Vec::new();
    }
    let (shell, bodies) = split_heredocs(command);
    let mut out = Vec::new();
    for mut segment in commands(&shell, &bodies) {
        let words = executable_words(&segment.words);
        let head = words.first().map(|s| basename(s)).unwrap_or_default();
        let source = if matches!(head, "sh" | "bash" | "zsh" | "dash" | "ksh") {
            if words.len() >= 3 && words[1].starts_with('-') && words[1].contains('c') {
                Some(words[2].clone())
            } else if !words.iter().skip(1).any(|s| !s.starts_with('-'))
                && !segment.bodies.is_empty()
            {
                Some(segment.bodies.join("\n"))
            } else {
                None
            }
        } else {
            None
        };
        if let Some(source) = source {
            let mut inner = walk(&source, depth + 1);
            if let Some(last) = inner.last_mut() {
                last.next.clone_from(&segment.next);
            }
            if let Some(first) = inner.first_mut() {
                segment.writes.append(&mut first.writes);
                first.writes = segment.writes;
                out.extend(inner);
                continue;
            }
        }
        out.push(segment);
    }
    out
}

pub(super) fn executable_words(words: &[String]) -> &[String] {
    let mut start = 0;
    while let Some(word) = words.get(start) {
        if prefix(word) || matches!(word.as_str(), "nice" | "rtk" | "proxy") {
            start += 1;
        } else if word == "timeout" {
            start += 1;
            while words.get(start).is_some_and(|s| {
                s.starts_with('-') || s.chars().next().is_some_and(|c| c.is_ascii_digit())
            }) {
                start += 1;
            }
        } else {
            break;
        }
    }
    &words[start..]
}

pub(super) fn python_script(segment: &ShellSegment) -> Option<String> {
    let words = executable_words(&segment.words);
    let head = words.first().map(|s| basename(s))?;
    if !pattern!(r"\Apython\d?(?:\.\d+)?\z").is_some_and(|re| re.is_match(head)) {
        return None;
    }
    if let Some(index) = words.iter().position(|s| s == "-c") {
        words.get(index + 1).cloned()
    } else if !words.iter().skip(1).any(|s| !s.starts_with('-')) && !segment.bodies.is_empty() {
        Some(segment.bodies.join("\n"))
    } else {
        None
    }
}

pub(super) fn segment_edit_targets(segment: &ShellSegment) -> Vec<String> {
    normalize_targets(
        segment
            .writes
            .iter()
            .filter(|p| source_file(p))
            .cloned()
            .chain(command_targets(segment)),
    )
}

fn command_targets(command: &ShellSegment) -> Vec<String> {
    let words = executable_words(&command.words);
    let Some(head) = words.first().map(|s| basename(s)) else {
        return Vec::new();
    };
    match head {
        "sed" | "perl" => inplace_files(words, head == "perl"),
        "tee" => words
            .iter()
            .skip(1)
            .filter(|s| !s.starts_with('-') && source_file(s))
            .cloned()
            .collect(),
        "apply_patch" => {
            let patch = format!("{}\n{}", command.bodies.join("\n"), words[1..].join(" "));
            let targets: Vec<String> =
                pattern!(r"(?m)^\*\*\* (?:Update|Add|Delete) File:\s*(.+?)\s*$")
                    .into_iter()
                    .flat_map(|re| re.captures_iter(&patch))
                    .filter_map(|c| c.get(1).map(|m| m.as_str().to_owned()))
                    .collect();
            if targets.is_empty() {
                vec!["(patch)".into()]
            } else {
                targets
            }
        }
        "patch" => vec!["(patch)".into()],
        "git" => match words.get(1).map(String::as_str) {
            Some("apply") => vec!["(patch)".into()],
            Some("checkout" | "restore") => {
                let start = words
                    .iter()
                    .position(|s| s == "--")
                    .map(|i| i + 1)
                    .or_else(|| (words[1] == "restore").then_some(2));
                start.map_or_else(Vec::new, |start| {
                    words[start..]
                        .iter()
                        .filter(|s| !s.starts_with('-'))
                        .cloned()
                        .collect()
                })
            }
            _ => Vec::new(),
        },
        _ => python_script(command).map_or_else(Vec::new, |script| py_writes(&script)),
    }
}

pub(super) fn basename(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default()
}

fn source_file(path: &str) -> bool {
    pattern!(r"(?i)\.(?:py|pyi|pyx|pxd|js|jsx|mjs|cjs|ts|tsx|rs|go|java|kt|kts|scala|c|cc|cpp|cxx|h|hh|hpp|rb|php|cs|swift|m|mm|sh|bash|zsh|lua|jl|ex|exs|erl|hs|ml|clj|sql|toml|ya?ml|json|cfg|ini|conf|md|rst|html?|css|scss|vue|svelte|xml|gradle|properties|in)$")
        .is_some_and(|re| re.is_match(path))
}

fn prefix(word: &str) -> bool {
    matches!(word, "sudo" | "command" | "env" | "time" | "nohup")
        || pattern!(r"^\w+=").is_some_and(|re| re.is_match(word))
}

fn inplace_files(words: &[String], perl: bool) -> Vec<String> {
    let inplace = words.iter().skip(1).any(|s| {
        (!perl && s.starts_with("--in-place"))
            || s.strip_prefix('-').is_some_and(|flags| {
                flags
                    .chars()
                    .take_while(char::is_ascii_alphabetic)
                    .any(|c| c == 'i')
            })
    });
    if !inplace {
        return Vec::new();
    }
    let mut files = Vec::new();
    let (mut have_script, mut skip) = (false, false);
    for word in &words[1..] {
        if skip {
            skip = false;
            have_script = true;
            continue;
        }
        if word.is_empty() || word.starts_with('-') {
            skip = if perl {
                word.starts_with('-') && word.ends_with('e')
            } else {
                matches!(word.as_str(), "-e" | "-f" | "--expression" | "--file")
            };
            continue;
        }
        if have_script {
            files.push(word.clone());
        } else {
            have_script = true;
        }
    }
    files
}

fn split_heredocs(command: &str) -> (String, Vec<String>) {
    let (mut shell, mut bodies, mut body) = (Vec::new(), Vec::new(), Vec::new());
    let mut tag: Option<String> = None;
    for line in command.split('\n') {
        if let Some(end) = &tag {
            if line.trim() == end {
                bodies.push(body.join("\n"));
                body.clear();
                tag = None;
            } else {
                body.push(line);
            }
            continue;
        }
        shell.push(line);
        tag = tokens(line).and_then(|parts| {
            parts
                .windows(2)
                .find(|pair| pair[0].operator && pair[0].text == "<<" && !pair[1].operator)
                .map(|pair| pair[1].text.trim_start_matches('-').to_owned())
        });
    }
    if tag.is_some() {
        bodies.push(body.join("\n"));
    }
    (shell.join("\n"), bodies)
}

#[derive(Debug)]
pub(super) struct Token {
    pub(super) text: String,
    pub(super) operator: bool,
}

// A small quote-aware lexer, not a shell interpreter. Quoted operators stay words.
pub(super) fn tokens(text: &str) -> Option<Vec<Token>> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut active = false;
    let mut quote = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else if c == '\\'
                && q == '"'
                && chars
                    .peek()
                    .is_some_and(|c| matches!(c, '"' | '\\' | '$' | '`' | '\n'))
            {
                if let Some(escaped) = chars.next() {
                    word.push(escaped);
                }
            } else {
                word.push(c);
            }
            continue;
        }
        if c == '#' && !active {
            // A comment ends at the next newline, which still separates commands.
            for ch in chars.by_ref() {
                if ch == '\n' {
                    out.push(Token {
                        text: "\n".into(),
                        operator: true,
                    });
                    break;
                }
            }
        } else if matches!(c, '\'' | '"') {
            quote = Some(c);
            active = true;
        } else if c == '\\' {
            active = true;
            word.push(chars.next()?);
        } else if c.is_whitespace() || "();<>|&".contains(c) {
            if active {
                out.push(Token {
                    text: std::mem::take(&mut word),
                    operator: false,
                });
                active = false;
            }
            if c == '\n' || !c.is_whitespace() {
                let mut op = c.to_string();
                if chars.peek().is_some_and(|n| {
                    matches!(
                        (c, *n),
                        ('&', '&' | '>')
                            | ('|', '|' | '&')
                            | (';', ';')
                            | ('<', '<' | '&' | '>')
                            | ('>', '>' | '&' | '|')
                    )
                }) {
                    if let Some(n) = chars.next() {
                        op.push(n);
                    }
                    if matches!(op.as_str(), "<<" | "&>") && chars.peek() == Some(&'>') {
                        chars.next();
                        op.push('>');
                    } else if op == "<<" && chars.peek() == Some(&'<') {
                        chars.next();
                        op.push('<');
                    }
                }
                out.push(Token {
                    text: op,
                    operator: true,
                });
            }
        } else {
            active = true;
            word.push(c);
        }
    }
    if quote.is_some() {
        return None;
    }
    if active {
        out.push(Token {
            text: word,
            operator: false,
        });
    }
    Some(out)
}

fn commands(shell: &str, bodies: &[String]) -> Vec<ShellSegment> {
    let streams =
        tokens(shell).map_or_else(|| shell.lines().filter_map(tokens).collect(), |t| vec![t]);
    let mut cmds = Vec::new();
    let mut bodies = bodies.iter();
    for stream in streams {
        let mut current = ShellSegment::default();
        let mut items = stream.into_iter();
        while let Some(t) = items.next() {
            if !t.operator {
                current.words.push(t.text);
                continue;
            }
            if matches!(
                t.text.as_str(),
                ">" | ">>" | ">|" | "&>" | "&>>" | "<" | "<<" | "<<<" | "<&" | ">&" | "<>"
            ) {
                if current
                    .words
                    .last()
                    .is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
                {
                    current.words.pop();
                }
                if let Some(operand) = items.next()
                    && !operand.operator
                {
                    if matches!(t.text.as_str(), ">" | ">>" | ">|" | "&>" | "&>>") {
                        current.writes.push(operand.text);
                    } else if t.text == "<<"
                        && let Some(body) = bodies.next()
                    {
                        current.bodies.push(body.clone());
                    }
                }
            } else if !current.words.is_empty() || !current.writes.is_empty() {
                current.next = t.text;
                cmds.push(std::mem::take(&mut current));
            }
        }
        if !current.words.is_empty() || !current.writes.is_empty() {
            cmds.push(current);
        }
    }
    cmds
}

fn py_writes(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut variables = Vec::new();
    if let Some(re) = pattern!(r#"open\(\s*['"]([^'"\n]+)['"]\s*,\s*['"]([^'"]*)['"]"#) {
        for c in re.captures_iter(text) {
            if c[2].chars().any(|ch| "wax+".contains(ch)) {
                paths.push(c[1].to_owned());
            }
        }
    }
    if let Some(re) = pattern!(r#"open\(\s*([A-Za-z_]\w*)\s*,\s*['"]([^'"]*)['"]"#) {
        for c in re.captures_iter(text) {
            if c[2].chars().any(|ch| "wax+".contains(ch)) {
                variables.push(c[1].to_owned());
            }
        }
    }
    if let Some(re) =
        pattern!(r#"(?:pathlib\.)?Path\(\s*['"]([^'"\n]+)['"]\s*\)\.write_(?:text|bytes)\("#)
    {
        paths.extend(re.captures_iter(text).map(|c| c[1].to_owned()));
    }
    if let Some(re) = pattern!(r"\b([A-Za-z_]\w*)\.write_(?:text|bytes)\(") {
        variables.extend(re.captures_iter(text).map(|c| c[1].to_owned()));
    }
    let mut seen = Vec::new();
    let literals = pattern!(r#"['"]([^'"\n]+)['"]"#);
    for var in variables {
        if seen.contains(&var) {
            continue;
        }
        seen.push(var.clone());
        let before = paths.len();
        let assignment = format!(
            r#"\b{}\s*=\s*(?:(?:pathlib\.)?Path\(\s*)?['"]([^'"\n]+)['"]"#,
            regex::escape(&var)
        );
        if let Ok(re) = regex::Regex::new(&assignment) {
            paths.extend(re.captures_iter(text).map(|c| c[1].to_owned()));
        }
        let loop_pattern = format!(
            r"\bfor\s+{}\s+in\s+[\[(]([^\])]*?)[\])]",
            regex::escape(&var)
        );
        if let Ok(re) = regex::Regex::new(&loop_pattern)
            && let Some(c) = re.captures(text)
            && let Some(literals) = literals
        {
            paths.extend(literals.captures_iter(&c[1]).map(|c| c[1].to_owned()));
        }
        if paths.len() == before {
            paths.push("(script)".into());
        }
    }
    paths
}
