//! The files a shell command plainly writes: the target of a redirect,
//! and the files `tee`, `sed -i`, `perl -i` and `sd` change. A path the
//! command builds at run time, through a variable it does not set or a
//! glob, is left out rather than guessed.

use std::collections::HashMap;

/// The files `command` writes, as absolute paths, when it runs in `cwd`.
pub(crate) fn writes(command: &str, cwd: Option<&str>) -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut dir = cwd.map(str::to_owned);
    let mut values: HashMap<String, String> = HashMap::new();
    let mut found = Vec::new();
    for words in commands(command) {
        let mut words = words.into_iter().peekable();
        // Assignments before a command set its environment; alone, they
        // set the shell's variables, which later paths may use.
        let mut rest = Vec::new();
        let mut assigning = true;
        let mut targets = Vec::new();
        while let Some(word) = words.next() {
            match word.kind {
                Word::Redirect => {
                    if let Some(target) = words.next() {
                        targets.push(target.text);
                    }
                    continue;
                }
                Word::Input => {
                    words.next();
                    continue;
                }
                Word::Text => {}
            }
            if assigning
                && let Some((name, value)) = word.text.split_once('=')
                && !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                if let Some(value) = expand(value, &values) {
                    values.insert(name.to_owned(), value);
                }
                continue;
            }
            assigning = false;
            rest.push(word.text);
        }
        let program = rest.first().map(String::as_str).unwrap_or_default();
        let args: Vec<&str> = rest.iter().skip(1).map(String::as_str).collect();
        match program.rsplit('/').next().unwrap_or(program) {
            "cd" => {
                if let Some(to) = args.first().and_then(|to| expand(to, &values)) {
                    dir = resolve(&to, dir.as_deref(), &home);
                }
            }
            "tee" => targets.extend(operands(&args).into_iter().map(str::to_owned)),
            "sed" | "gsed" | "perl" if args.iter().any(|arg| in_place(arg)) => {
                // The script is the first operand unless -e gave it.
                let given = args
                    .iter()
                    .any(|arg| *arg == "-e" || *arg == "--expression");
                let files = operands(&args);
                let skip = usize::from(!given);
                targets.extend(files.into_iter().skip(skip).map(str::to_owned));
            }
            // sd FIND REPLACE [FILE…]
            "sd" => targets.extend(operands(&args).into_iter().skip(2).map(str::to_owned)),
            _ => {}
        }
        for target in targets {
            if target == "/dev/null" || target.starts_with('&') || target.starts_with("/dev/") {
                continue;
            }
            if let Some(path) =
                expand(&target, &values).and_then(|path| resolve(&path, dir.as_deref(), &home))
                && !found.contains(&path)
            {
                found.push(path);
            }
        }
    }
    found
}

/// Whether `arg` asks sed or perl to change its files in place: `-i`,
/// alone or among other one-letter flags such as perl's `-pi`.
fn in_place(arg: &str) -> bool {
    arg == "--in-place"
        || arg.starts_with("--in-place=")
        || (arg.starts_with('-') && !arg.starts_with("--") && arg.contains('i'))
}

/// The arguments that are not options, and not the values of the options
/// that take one.
fn operands<'arg>(args: &[&'arg str]) -> Vec<&'arg str> {
    let mut out = Vec::new();
    let mut skip = false;
    for arg in args {
        if skip {
            skip = false;
            continue;
        }
        if *arg == "-e" || *arg == "--expression" || *arg == "-f" {
            skip = true;
            continue;
        }
        if arg.starts_with('-') && arg.len() > 1 {
            continue;
        }
        out.push(*arg);
    }
    out
}

/// `text` with the variables `values` holds put in; `None` when it uses
/// one the command did not set, or holds a glob or a command.
fn expand(text: &str, values: &HashMap<String, String>) -> Option<String> {
    if text.contains(['*', '?', '`']) || text.contains("$(") {
        return None;
    }
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        let braced = chars.peek() == Some(&'{');
        if braced {
            chars.next();
        }
        let mut name = String::new();
        while let Some(&next) = chars.peek() {
            if next.is_ascii_alphanumeric() || next == '_' {
                name.push(next);
                chars.next();
            } else {
                break;
            }
        }
        if braced && chars.next() != Some('}') {
            return None;
        }
        let value = match name.as_str() {
            "HOME" => std::env::var("HOME").ok()?,
            _ => values.get(&name)?.clone(),
        };
        out.push_str(&value);
    }
    Some(out)
}

/// `path` made absolute from `dir`, with `~` for the home folder and the
/// `.` and `..` parts worked out.
fn resolve(path: &str, dir: Option<&str>, home: &str) -> Option<String> {
    let full = if let Some(rest) = path.strip_prefix("~/") {
        format!("{home}/{rest}")
    } else if path == "~" {
        home.to_owned()
    } else if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("{}/{path}", dir?)
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in full.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    Some(format!("/{}", parts.join("/")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Word {
    Text,
    /// `>`, `>>`, `&>` or `N>`: the next word is a file the command writes.
    Redirect,
    /// `<`: the next word is a file the command reads.
    Input,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Token {
    kind: Word,
    text: String,
}

/// The simple commands of `command`, each as its words with the quotes
/// taken off, split at `;`, `&&`, `||`, `|`, `&` and line ends. A here-
/// document's body is skipped: it is the command's input, not its words.
fn commands(command: &str) -> Vec<Vec<Token>> {
    let mut out = Vec::new();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut heredocs: Vec<String> = Vec::new();
    let mut chars = command.chars().peekable();
    let end_word = |word: &mut String, started: &mut bool, words: &mut Vec<Token>| {
        if *started {
            words.push(Token {
                kind: Word::Text,
                text: std::mem::take(word),
            });
            *started = false;
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                started = true;
                for next in chars.by_ref() {
                    if next == '\'' {
                        break;
                    }
                    word.push(next);
                }
            }
            '"' => {
                started = true;
                while let Some(next) = chars.next() {
                    match next {
                        '"' => break,
                        '\\' => {
                            if let Some(escaped) = chars.next() {
                                word.push(escaped);
                            }
                        }
                        next => word.push(next),
                    }
                }
            }
            '\\' => {
                started = true;
                if let Some(next) = chars.next()
                    && next != '\n'
                {
                    word.push(next);
                }
            }
            '#' if !started => {
                // A comment runs to the line's end.
                while chars.peek().is_some_and(|next| *next != '\n') {
                    chars.next();
                }
            }
            '<' if chars.peek() == Some(&'<') => {
                end_word(&mut word, &mut started, &mut words);
                chars.next();
                if chars.peek() == Some(&'<') {
                    // A here-string: its word is input.
                    chars.next();
                    continue;
                }
                if chars.peek() == Some(&'-') {
                    chars.next();
                }
                while chars.peek() == Some(&' ') {
                    chars.next();
                }
                let mut delimiter = String::new();
                while let Some(&next) = chars.peek() {
                    if next.is_whitespace() || matches!(next, ';' | '|' | '&' | '<' | '>' | ')') {
                        break;
                    }
                    chars.next();
                    if next != '\'' && next != '"' {
                        delimiter.push(next);
                    }
                }
                heredocs.push(delimiter);
            }
            '<' => {
                end_word(&mut word, &mut started, &mut words);
                words.push(Token {
                    kind: Word::Input,
                    text: String::new(),
                });
            }
            '>' => {
                // `2>&1` and `>&2` copy a stream; they write no file.
                let number = !word.is_empty() && word.chars().all(|c| c.is_ascii_digit());
                if number {
                    word.clear();
                    started = false;
                }
                end_word(&mut word, &mut started, &mut words);
                if chars.peek() == Some(&'>') {
                    chars.next();
                }
                if chars.peek() == Some(&'&') {
                    chars.next();
                    while chars
                        .peek()
                        .is_some_and(|next| next.is_ascii_digit() || *next == '-')
                    {
                        chars.next();
                    }
                    continue;
                }
                if chars.peek() == Some(&'|') {
                    chars.next();
                }
                words.push(Token {
                    kind: Word::Redirect,
                    text: String::new(),
                });
            }
            '&' if chars.peek() == Some(&'>') => {
                end_word(&mut word, &mut started, &mut words);
                chars.next();
                if chars.peek() == Some(&'>') {
                    chars.next();
                }
                words.push(Token {
                    kind: Word::Redirect,
                    text: String::new(),
                });
            }
            ';' | '|' | '&' | '\n' | '(' | ')' => {
                end_word(&mut word, &mut started, &mut words);
                if !words.is_empty() {
                    out.push(std::mem::take(&mut words));
                }
                if c == '\n' {
                    // Each here-document's body follows the line that
                    // opened it, up to its delimiter on a line alone.
                    for delimiter in std::mem::take(&mut heredocs) {
                        let mut line = String::new();
                        for next in chars.by_ref() {
                            if next == '\n' {
                                if line.trim() == delimiter {
                                    break;
                                }
                                line.clear();
                            } else {
                                line.push(next);
                            }
                        }
                    }
                }
            }
            c if c.is_whitespace() => end_word(&mut word, &mut started, &mut words),
            c => {
                started = true;
                word.push(c);
            }
        }
    }
    end_word(&mut word, &mut started, &mut words);
    if !words.is_empty() {
        out.push(words);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::writes;

    fn written(command: &str) -> Vec<String> {
        writes(command, Some("/work/app"))
    }

    #[test]
    fn redirects_write_their_targets() {
        assert_eq!(written("echo hi > notes.txt"), ["/work/app/notes.txt"]);
        assert_eq!(written("printf x >> /tmp/log.txt 2>&1"), ["/tmp/log.txt"]);
        assert_eq!(written("make 2> errors.txt"), ["/work/app/errors.txt"]);
        assert_eq!(written("ls >/dev/null 2>&1"), Vec::<String>::new());
        assert_eq!(written("cmd 2>&1 | rg x"), Vec::<String>::new());
    }

    #[test]
    fn cd_moves_where_relative_paths_land() {
        assert_eq!(
            written("cd ../other && sed -i 's/a/b/' src/lib.rs; tee out.log < in.log"),
            ["/work/other/src/lib.rs", "/work/other/out.log"]
        );
    }

    #[test]
    fn in_place_editors_write_their_files() {
        assert_eq!(
            written("sd -F 'old' 'new' a.md b.md"),
            ["/work/app/a.md", "/work/app/b.md"]
        );
        assert_eq!(written("sed -n 1,5p a.md"), Vec::<String>::new());
        assert_eq!(written("sed -i -e 's/x/y/' c.md"), ["/work/app/c.md"]);
        assert_eq!(written("perl -pi -e 's/x/y/' d.md"), ["/work/app/d.md"]);
    }

    #[test]
    fn variables_the_command_sets_are_put_in() {
        assert_eq!(
            written(
                "D=/tmp/box; cat > \"$D/plan.md\" <<'EOF'\nnot > a/file\nEOF\necho done > ${D}/done"
            ),
            ["/tmp/box/plan.md", "/tmp/box/done"]
        );
        assert_eq!(written("cat > $UNSET/x.md"), Vec::<String>::new());
        assert_eq!(written("cat > out-*.md"), Vec::<String>::new());
    }
}
