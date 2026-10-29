// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/loopguard/shell_edits.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Heuristic shell-write detection; no shell execution or filesystem access.
/// Ordered unique basenames written by a shell command, as in TE.
/// Unresolved script and patch targets use `(script)` and `(patch)`.
#[must_use]
pub fn shell_edit_targets(command: &str) -> Vec<String> {
    let command = shell_script(command);
    let (shell, bodies) = split_heredocs(&command);
    let (commands, writes) = commands(&shell);
    let patch_files = pattern!(r"(?m)^\*\*\* (?:Update|Add|Delete) File:\s*(.+?)\s*$");
    let mut found: Vec<String> = writes.into_iter().filter(|s| source_file(s)).collect();
    for mut words in commands {
        while words.first().is_some_and(|s| prefix(s)) {
            words.remove(0);
        }
        let Some(head) = words.first().map(|s| basename(s)) else {
            continue;
        };
        match head {
            "sed" | "perl" => found.extend(inplace_files(&words, head == "perl")),
            "tee" => found.extend(
                words
                    .iter()
                    .skip(1)
                    .filter(|s| !s.starts_with('-') && source_file(s))
                    .cloned(),
            ),
            "apply_patch" => {
                let patch = format!("{}\n{}", bodies.join("\n"), words[1..].join(" "));
                let targets: Vec<String> = patch_files
                    .into_iter()
                    .flat_map(|re| re.captures_iter(&patch))
                    .filter_map(|c| c.get(1).map(|m| m.as_str().to_owned()))
                    .collect();
                if targets.is_empty() {
                    found.push("(patch)".into());
                } else {
                    found.extend(targets);
                }
            }
            "patch" => found.push("(patch)".into()),
            "git" => match words.get(1).map(String::as_str) {
                Some("apply") => found.push("(patch)".into()),
                Some("checkout" | "restore") => {
                    let start = words
                        .iter()
                        .position(|s| s == "--")
                        .map(|i| i + 1)
                        .or_else(|| (words[1] == "restore").then_some(2));
                    if let Some(start) = start {
                        found.extend(
                            words[start..]
                                .iter()
                                .filter(|s| !s.starts_with('-'))
                                .cloned(),
                        );
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    if pattern!(r"(?:^|[\s/;&|(])python\d?(?:\.\d+)?\b").is_some_and(|re| re.is_match(&shell)) {
        found.extend(py_writes(&format!("{shell}\n{}", bodies.join("\n"))));
    }
    let mut names = Vec::new();
    for path in found {
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
    let heredoc_start = pattern!(r#"<<-?\s*['"]?([A-Za-z_]\w*)['"]?"#);
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
        tag = heredoc_start
            .and_then(|re| re.captures(line))
            .and_then(|c| c.get(1).map(|m| m.as_str().to_owned()));
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
        if matches!(c, '\'' | '"') {
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

pub(super) fn shell_script(command: &str) -> String {
    if let Some(parts) = tokens(command)
        && parts.len() >= 3
        && matches!(
            basename(&parts[0].text),
            "sh" | "bash" | "zsh" | "dash" | "ksh"
        )
        && parts[1].text.starts_with('-')
        && parts[1].text.contains('c')
    {
        return parts[2].text.clone();
    }
    command.to_owned()
}

fn commands(shell: &str) -> (Vec<Vec<String>>, Vec<String>) {
    let streams =
        tokens(shell).map_or_else(|| shell.lines().filter_map(tokens).collect(), |t| vec![t]);
    let (mut cmds, mut writes) = (Vec::new(), Vec::new());
    for stream in streams {
        let mut current: Vec<String> = Vec::new();
        let mut items = stream.into_iter();
        while let Some(t) = items.next() {
            if !t.operator {
                current.push(t.text);
                continue;
            }
            if matches!(
                t.text.as_str(),
                ">" | ">>" | ">|" | "&>" | "&>>" | "<" | "<<" | "<<<" | "<&" | ">&" | "<>"
            ) {
                if current
                    .last()
                    .is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
                {
                    current.pop();
                }
                if let Some(operand) = items.next()
                    && !operand.operator
                    && matches!(t.text.as_str(), ">" | ">>" | ">|" | "&>" | "&>>")
                {
                    writes.push(operand.text);
                }
            } else if !current.is_empty() {
                cmds.push(std::mem::take(&mut current));
            }
        }
        if !current.is_empty() {
            cmds.push(current);
        }
    }
    (cmds, writes)
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
