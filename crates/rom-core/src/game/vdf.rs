#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Str(String),
    Obj(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(children) => children
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v),
            Value::Str(_) => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            Value::Obj(_) => None,
        }
    }

    pub fn children(&self) -> &[(String, Value)] {
        match self {
            Value::Obj(children) => children,
            Value::Str(_) => &[],
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Str(String),
    Open,
    Close,
}

fn tokenize(input: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            c if c.is_whitespace() => {
                chars.next();
            }
            '{' => {
                chars.next();
                tokens.push(Token::Open);
            }
            '}' => {
                chars.next();
                tokens.push(Token::Close);
            }
            '/' => {
                chars.next();
                chars.next_if_eq(&'/')?;
                while chars.next_if(|&c| c != '\n').is_some() {}
            }
            '[' => while chars.next().is_some_and(|c| c != ']') {},
            '"' => {
                chars.next();
                let mut s = String::new();
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => match chars.next()? {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            other => s.push(other),
                        },
                        other => s.push(other),
                    }
                }
                tokens.push(Token::Str(s));
            }
            _ => {
                let mut s = String::new();
                while let Some(c) =
                    chars.next_if(|&c| !c.is_whitespace() && !matches!(c, '"' | '{' | '}'))
                {
                    s.push(c);
                }
                tokens.push(Token::Str(s));
            }
        }
    }
    Some(tokens)
}

pub fn parse(input: &str) -> Option<Value> {
    let tokens = tokenize(input)?;
    let mut iter = tokens.into_iter();
    let root = parse_pairs(&mut iter, false)?;
    Some(Value::Obj(root))
}

fn parse_pairs(
    iter: &mut impl Iterator<Item = Token>,
    nested: bool,
) -> Option<Vec<(String, Value)>> {
    let mut pairs = Vec::new();
    loop {
        let key = match iter.next() {
            Some(Token::Str(key)) => key,
            Some(Token::Close) if nested => return Some(pairs),
            None if !nested => return Some(pairs),
            _ => return None,
        };
        let value = match iter.next()? {
            Token::Str(s) => Value::Str(s),
            Token::Open => Value::Obj(parse_pairs(iter, true)?),
            Token::Close => return None,
        };
        pairs.push((key, value));
    }
}
