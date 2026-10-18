//! GDScript tokenizer with Python-style INDENT/DEDENT block structure.

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),
    Newl,
    Indent,
    Dedent,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Dot,
    Assign,   // =
    PlusEq,   // +=
    MinusEq,  // -=
    StarEq,   // *=
    SlashEq,  // /=
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,  // ==
    Neq, // !=
    Lt,
    Le,
    Gt,
    Ge,
    Eof,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub line: usize,
}

const TAB_WIDTH: usize = 4;

pub fn lex(src: &str) -> Result<Vec<Token>, String> {
    let mut out: Vec<Token> = Vec::new();
    let mut indents: Vec<usize> = vec![0];
    let mut paren_depth = 0usize;

    for (li, raw) in src.lines().enumerate() {
        let line_no = li + 1;
        let chars: Vec<char> = raw.chars().collect();

        if paren_depth == 0 {
            // indentation handling (indent width in spaces, slicing in chars)
            let mut ind = 0usize; // space-equivalent width
            let mut lead = 0usize; // number of leading whitespace chars
            for &c in &chars {
                if c == ' ' {
                    ind += 1;
                    lead += 1;
                } else if c == '\t' {
                    ind += TAB_WIDTH;
                    lead += 1;
                } else {
                    break;
                }
            }
            let body: String = chars[lead.min(chars.len())..].iter().collect();
            let trimmed = body.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue; // blank / comment-only line: no Newl, no indent change
            }
            if ind > *indents.last().unwrap() {
                indents.push(ind);
                out.push(Token { tok: Tok::Indent, line: line_no });
            } else {
                while ind < *indents.last().unwrap() {
                    indents.pop();
                    out.push(Token { tok: Tok::Dedent, line: line_no });
                }
                if ind != *indents.last().unwrap() {
                    return Err(format!("line {line_no}: inconsistent indentation"));
                }
            }
        }

        // scan tokens
        let mut i = 0usize;
        while i < chars.len() {
            let c = chars[i];
            match c {
                ' ' | '\t' | '\r' => {
                    i += 1;
                }
                '#' => break, // comment to end of line
                ';' if paren_depth == 0 => {
                    // GDScript allows `stmt; stmt` on one line — treat the
                    // semicolon as a statement boundary (virtual newline)
                    out.push(Token { tok: Tok::Newl, line: line_no });
                    i += 1;
                }
                '"' => {
                    let (s, ni) = read_string(&chars, i, line_no)?;
                    out.push(Token { tok: Tok::Str(s), line: line_no });
                    i = ni;
                }
                '0'..='9' => {
                    let (tok, ni) = read_number(&chars, i);
                    out.push(Token { tok, line: line_no });
                    i = ni;
                }
                c if is_ident_start(c) => {
                    let mut j = i;
                    while j < chars.len() && is_ident_char(chars[j]) {
                        j += 1;
                    }
                    let s: String = chars[i..j].iter().collect();
                    out.push(Token { tok: Tok::Ident(s), line: line_no });
                    i = j;
                }
                '(' => {
                    paren_depth += 1;
                    out.push(Token { tok: Tok::LParen, line: line_no });
                    i += 1;
                }
                ')' => {
                    paren_depth = paren_depth.saturating_sub(1);
                    out.push(Token { tok: Tok::RParen, line: line_no });
                    i += 1;
                }
                '[' => {
                    out.push(Token { tok: Tok::LBracket, line: line_no });
                    i += 1;
                }
                ']' => {
                    out.push(Token { tok: Tok::RBracket, line: line_no });
                    i += 1;
                }
                ',' => {
                    out.push(Token { tok: Tok::Comma, line: line_no });
                    i += 1;
                }
                ':' => {
                    out.push(Token { tok: Tok::Colon, line: line_no });
                    i += 1;
                }
                '.' => {
                    out.push(Token { tok: Tok::Dot, line: line_no });
                    i += 1;
                }
                '=' => {
                    if i + 1 < chars.len() && chars[i + 1] == '=' {
                        out.push(Token { tok: Tok::Eq, line: line_no });
                        i += 2;
                    } else {
                        out.push(Token { tok: Tok::Assign, line: line_no });
                        i += 1;
                    }
                }
                '!' => {
                    if i + 1 < chars.len() && chars[i + 1] == '=' {
                        out.push(Token { tok: Tok::Neq, line: line_no });
                        i += 2;
                    } else {
                        return Err(format!("line {line_no}: unexpected '!'"));
                    }
                }
                '<' => {
                    if i + 1 < chars.len() && chars[i + 1] == '=' {
                        out.push(Token { tok: Tok::Le, line: line_no });
                        i += 2;
                    } else {
                        out.push(Token { tok: Tok::Lt, line: line_no });
                        i += 1;
                    }
                }
                '>' => {
                    if i + 1 < chars.len() && chars[i + 1] == '=' {
                        out.push(Token { tok: Tok::Ge, line: line_no });
                        i += 2;
                    } else {
                        out.push(Token { tok: Tok::Gt, line: line_no });
                        i += 1;
                    }
                }
                '+' => {
                    if i + 1 < chars.len() && chars[i + 1] == '=' {
                        out.push(Token { tok: Tok::PlusEq, line: line_no });
                        i += 2;
                    } else {
                        out.push(Token { tok: Tok::Plus, line: line_no });
                        i += 1;
                    }
                }
                '-' => {
                    if i + 1 < chars.len() && chars[i + 1] == '=' {
                        out.push(Token { tok: Tok::MinusEq, line: line_no });
                        i += 2;
                    } else {
                        out.push(Token { tok: Tok::Minus, line: line_no });
                        i += 1;
                    }
                }
                '*' => {
                    if i + 1 < chars.len() && chars[i + 1] == '=' {
                        out.push(Token { tok: Tok::StarEq, line: line_no });
                        i += 2;
                    } else {
                        out.push(Token { tok: Tok::Star, line: line_no });
                        i += 1;
                    }
                }
                '/' => {
                    if i + 1 < chars.len() && chars[i + 1] == '=' {
                        out.push(Token { tok: Tok::SlashEq, line: line_no });
                        i += 2;
                    } else {
                        out.push(Token { tok: Tok::Slash, line: line_no });
                        i += 1;
                    }
                }
                '%' => {
                    out.push(Token { tok: Tok::Percent, line: line_no });
                    i += 1;
                }
                other => {
                    return Err(format!(
                        "line {line_no}: unexpected character '{other}'"
                    ));
                }
            }
        }

        if paren_depth == 0 {
            out.push(Token { tok: Tok::Newl, line: line_no });
        }
    }

    while indents.len() > 1 {
        indents.pop();
        let line = src.lines().count();
        out.push(Token { tok: Tok::Dedent, line });
    }
    out.push(Token { tok: Tok::Eof, line: src.lines().count() });
    Ok(out)
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn read_string(chars: &[char], start: usize, line: usize) -> Result<(String, usize), String> {
    let mut i = start + 1;
    let mut s = String::new();
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            return Ok((s, i + 1));
        }
        if c == '\\' && i + 1 < chars.len() {
            let e = chars[i + 1];
            match e {
                'n' => s.push('\n'),
                't' => s.push('\t'),
                '"' => s.push('"'),
                '\\' => s.push('\\'),
                other => {
                    s.push('\\');
                    s.push(other);
                }
            }
            i += 2;
            continue;
        }
        if c == '\n' {
            break;
        }
        s.push(c);
        i += 1;
    }
    Err(format!("line {line}: unterminated string"))
}

fn read_number(chars: &[char], start: usize) -> (Tok, usize) {
    let mut i = start;
    let mut is_float = false;
    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
        i += 1;
    }
    if i < chars.len() && chars[i] == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
        is_float = true;
        i += 1;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
        let mut j = i + 1;
        if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
            j += 1;
        }
        if j < chars.len() && chars[j].is_ascii_digit() {
            is_float = true;
            i = j;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
        }
    }
    let s: String = chars[start..i].iter().filter(|c| **c != '_').collect();
    if is_float {
        (Tok::Float(s.parse::<f64>().unwrap_or(0.0)), i)
    } else {
        (Tok::Int(s.parse::<i64>().unwrap_or(0)), i)
    }
}
