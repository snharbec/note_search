use std::fmt;

/// A parsed query expression for Obsidian-like search syntax.
///
/// Supports:
/// - Plain words → text search
/// - `[[link]]` → link search
/// - `#tag` → tag search
/// - `[attribute]` → attribute exists
/// - `[attribute:value]` → attribute has value
/// - `(expr OR expr)` → OR grouping
/// - Implicit AND between all terms at the same level
#[derive(Debug, Clone, PartialEq)]
pub enum QueryExpr {
    /// A plain text word to search for in note title/body
    Text(String),
    /// A wiki link `[[NoteName]]`
    Link(String),
    /// A tag `#tagname`
    Tag(String),
    /// An attribute `[attr]` or `[attr:value]`
    Attribute { key: String, value: Option<String> },
    /// All sub-expressions must match (AND)
    And(Vec<QueryExpr>),
    /// At least one sub-expression must match (OR)
    Or(Vec<QueryExpr>),
}

impl fmt::Display for QueryExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryExpr::Text(s) => write!(f, "{}", s),
            QueryExpr::Link(s) => write!(f, "[[{}]]", s),
            QueryExpr::Tag(s) => write!(f, "#{}", s),
            QueryExpr::Attribute { key, value: Some(v) } => write!(f, "[{}:{}]", key, v),
            QueryExpr::Attribute { key, value: None } => write!(f, "[{}]", key),
            QueryExpr::And(exprs) => {
                let parts: Vec<String> = exprs.iter().map(|e| e.to_string()).collect();
                write!(f, "({})", parts.join(" "))
            }
            QueryExpr::Or(exprs) => {
                let parts: Vec<String> = exprs.iter().map(|e| e.to_string()).collect();
                write!(f, "({})", parts.join(" OR "))
            }
        }
    }
}

// ---- Tokenizer ----

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Word(String),
    Tag(String),
    Link(String),
    Attribute { key: String, value: Option<String> },
    OpenParen,
    CloseParen,
    Or,
}

struct Tokenizer {
    chars: Vec<char>,
    pos: usize,
}

impl Tokenizer {
    fn new(input: &str) -> Self {
        Tokenizer {
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied();
        self.pos += 1;
        c
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn tokenize(&mut self) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::new();

        loop {
            self.skip_whitespace();
            match self.peek() {
                None => break,
                Some('(') => {
                    self.advance();
                    tokens.push(Token::OpenParen);
                }
                Some(')') => {
                    self.advance();
                    tokens.push(Token::CloseParen);
                }
                Some('#') => {
                    self.advance();
                    let mut tag = String::new();
                    while let Some(c) = self.peek() {
                        if c.is_alphanumeric()
                            || c == '_'
                            || c == '/'
                            || c == '-'
                            || c == '.'
                        {
                            tag.push(c);
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    if tag.is_empty() {
                        return Err("Empty tag after #".to_string());
                    }
                    tokens.push(Token::Tag(tag));
                }
                Some('@') => {
                    self.advance();
                    let mut name = String::new();
                    while let Some(c) = self.peek() {
                        if c.is_alphanumeric()
                            || c == '_'
                            || c == '/'
                            || c == '-'
                            || c == '.'
                        {
                            name.push(c);
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    if name.is_empty() {
                        return Err("Empty name after @".to_string());
                    }
                    tokens.push(Token::Link(name));
                }
                Some('[') => {
                    // Check for [[link]]
                    if self.pos + 1 < self.chars.len() && self.chars[self.pos + 1] == '[' {
                        self.advance(); // consume first [
                        self.advance(); // consume second [
                        let mut link = String::new();
                        loop {
                            match self.peek() {
                                None => {
                                    return Err("Unclosed [[ link".to_string());
                                }
                                Some(']') => {
                                    if self.pos + 1 < self.chars.len()
                                        && self.chars[self.pos + 1] == ']'
                                    {
                                        self.advance(); // consume first ]
                                        self.advance(); // consume second ]
                                        break;
                                    } else {
                                        link.push(']');
                                        self.advance();
                                    }
                                }
                                Some(c) => {
                                    link.push(c);
                                    self.advance();
                                }
                            }
                        }
                        if link.is_empty() {
                            return Err("Empty link [[]]".to_string());
                        }
                        tokens.push(Token::Link(link.trim().to_string()));
                    } else {
                        // Single bracket: [attribute] or [attribute:value]
                        self.advance(); // consume [
                        let mut content = String::new();
                        loop {
                            match self.peek() {
                                None => {
                                    return Err("Unclosed [ attribute bracket".to_string());
                                }
                                Some(']') => {
                                    self.advance(); // consume ]
                                    break;
                                }
                                Some(c) => {
                                    content.push(c);
                                    self.advance();
                                }
                            }
                        }
                        if content.is_empty() {
                            return Err("Empty attribute brackets []".to_string());
                        }
                        // Split on ':' for key:value
                        if let Some(colon_pos) = content.find(':') {
                            let key = content[..colon_pos].trim().to_string();
                            let value = content[colon_pos + 1..].trim().to_string();
                            if key.is_empty() {
                                return Err("Empty attribute key in [:value]".to_string());
                            }
                            tokens.push(Token::Attribute {
                                key,
                                value: Some(value),
                            });
                        } else {
                            tokens.push(Token::Attribute {
                                key: content.trim().to_string(),
                                value: None,
                            });
                        }
                    }
                }
                Some(c) if c.is_alphanumeric() || c == '"' || c == '-' || c == '_' || c == '.' => {
                    let mut word = String::new();
                    if c == '"' {
                        // Quoted string
                        self.advance(); // consume opening quote
                        loop {
                            match self.peek() {
                                None => return Err("Unclosed quote".to_string()),
                                Some('"') => {
                                    self.advance(); // consume closing quote
                                    break;
                                }
                                Some(c) => {
                                    word.push(c);
                                    self.advance();
                                }
                            }
                        }
                    } else {
                        while let Some(c) = self.peek() {
                            if c.is_whitespace() || c == '(' || c == ')' || c == '#' {
                                break;
                            }
                            word.push(c);
                            self.advance();
                        }
                    }
                    // Check if this is the OR operator (case-insensitive)
                    if word.eq_ignore_ascii_case("or") {
                        tokens.push(Token::Or);
                    } else {
                        tokens.push(Token::Word(word));
                    }
                }
                Some(_c) => {
                    // Any other character - treat as a word character
                    let mut word = String::new();
                    while let Some(c) = self.peek() {
                        if c.is_whitespace() || c == '(' || c == ')' {
                            break;
                        }
                        word.push(c);
                        self.advance();
                    }
                    tokens.push(Token::Word(word));
                }
            }
        }

        Ok(tokens)
    }
}

// ---- Parser ----

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<&Token> {
        let token = self.tokens.get(self.pos);
        self.pos += 1;
        token
    }

    /// Parse the full query
    /// Grammar:
    ///   query     = or_expr
    ///   or_expr   = and_expr ("OR" and_expr)*
    ///   and_expr  = term+
    ///   term      = word | tag | link | attribute | "(" or_expr ")"
    fn parse(&mut self) -> Result<QueryExpr, String> {
        let expr = self.parse_or()?;
        if self.peek().is_some() {
            return Err(format!(
                "Unexpected token after end of expression: {:?}",
                self.peek()
            ));
        }
        Ok(expr)
    }

    fn parse_or(&mut self) -> Result<QueryExpr, String> {
        let mut exprs = Vec::new();
        exprs.push(self.parse_and()?);

        while self.peek() == Some(&Token::Or) {
            self.advance(); // consume OR
            exprs.push(self.parse_and()?);
        }

        if exprs.len() == 1 {
            Ok(exprs.into_iter().next().unwrap())
        } else {
            Ok(QueryExpr::Or(exprs))
        }
    }

    fn parse_and(&mut self) -> Result<QueryExpr, String> {
        let mut exprs = Vec::new();

        loop {
            match self.peek() {
                None | Some(Token::CloseParen) | Some(Token::Or) => break,
                _ => {
                    exprs.push(self.parse_term()?);
                }
            }
        }

        if exprs.is_empty() {
            return Err("Expected a term but found nothing".to_string());
        }

        if exprs.len() == 1 {
            Ok(exprs.into_iter().next().unwrap())
        } else {
            Ok(QueryExpr::And(exprs))
        }
    }

    fn parse_term(&mut self) -> Result<QueryExpr, String> {
        match self.advance() {
            Some(Token::Word(w)) => Ok(QueryExpr::Text(w.clone())),
            Some(Token::Tag(t)) => Ok(QueryExpr::Tag(t.clone())),
            Some(Token::Link(l)) => Ok(QueryExpr::Link(l.clone())),
            Some(Token::Attribute { key, value }) => Ok(QueryExpr::Attribute {
                key: key.clone(),
                value: value.clone(),
            }),
            Some(Token::OpenParen) => {
                let expr = self.parse_or()?;
                match self.advance() {
                    Some(Token::CloseParen) => Ok(expr),
                    Some(t) => Err(format!("Expected ')' but found {:?}", t)),
                    None => Err("Unclosed parentheses".to_string()),
                }
            }
            Some(t) => Err(format!("Unexpected token: {:?}", t)),
            None => Err("Unexpected end of input".to_string()),
        }
    }
}

/// Parse an Obsidian-like query string into a QueryExpr.
///
/// Examples:
/// - `"word1 word2 [[note1]] #tag1"` → And([Text("word1"), Text("word2"), Link("note1"), Tag("tag1")])
/// - `"[status]"` → Attribute { key: "status", value: None }
/// - `"[type:meeting]"` → Attribute { key: "type", value: Some("meeting") }
/// - `"word1 (word2 OR word3)"` → And([Text("word1"), Or([Text("word2"), Text("word3")])])
pub fn parse_query(input: &str) -> Result<QueryExpr, String> {
    let mut tokenizer = Tokenizer::new(input);
    let tokens = tokenizer.tokenize()?;
    let mut parser = Parser::new(tokens);
    parser.parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_word() {
        let expr = parse_query("hello").unwrap();
        assert_eq!(expr, QueryExpr::Text("hello".to_string()));
    }

    #[test]
    fn test_multiple_words() {
        let expr = parse_query("hello world").unwrap();
        assert_eq!(
            expr,
            QueryExpr::And(vec![
                QueryExpr::Text("hello".to_string()),
                QueryExpr::Text("world".to_string()),
            ])
        );
    }

    #[test]
    fn test_tag() {
        let expr = parse_query("#tag1").unwrap();
        assert_eq!(expr, QueryExpr::Tag("tag1".to_string()));
    }

    #[test]
    fn test_link() {
        let expr = parse_query("[[Note Name]]").unwrap();
        assert_eq!(expr, QueryExpr::Link("Note Name".to_string()));
    }

    #[test]
    fn test_mixed() {
        let expr = parse_query("word1 [[note1]] #tag1 word2").unwrap();
        assert_eq!(
            expr,
            QueryExpr::And(vec![
                QueryExpr::Text("word1".to_string()),
                QueryExpr::Link("note1".to_string()),
                QueryExpr::Tag("tag1".to_string()),
                QueryExpr::Text("word2".to_string()),
            ])
        );
    }

    #[test]
    fn test_or_simple() {
        let expr = parse_query("(word1 OR word2)").unwrap();
        assert_eq!(
            expr,
            QueryExpr::Or(vec![
                QueryExpr::Text("word1".to_string()),
                QueryExpr::Text("word2".to_string()),
            ])
        );
    }

    #[test]
    fn test_and_with_or() {
        let expr = parse_query("word1 (word2 OR word3)").unwrap();
        assert_eq!(
            expr,
            QueryExpr::And(vec![
                QueryExpr::Text("word1".to_string()),
                QueryExpr::Or(vec![
                    QueryExpr::Text("word2".to_string()),
                    QueryExpr::Text("word3".to_string()),
                ]),
            ])
        );
    }

    #[test]
    fn test_complex() {
        let expr = parse_query("word1 [[note1]] #tag1 (word2 OR word3)").unwrap();
        assert_eq!(
            expr,
            QueryExpr::And(vec![
                QueryExpr::Text("word1".to_string()),
                QueryExpr::Link("note1".to_string()),
                QueryExpr::Tag("tag1".to_string()),
                QueryExpr::Or(vec![
                    QueryExpr::Text("word2".to_string()),
                    QueryExpr::Text("word3".to_string()),
                ]),
            ])
        );
    }

    #[test]
    fn test_nested_parens() {
        let expr = parse_query("word1 ((word2 OR word3) word4)").unwrap();
        // The inner (word2 OR word3) is parsed as Or, then AND with word4
        // Since there's no explicit AND keyword, adjacent terms are ANDed
        assert_eq!(
            expr,
            QueryExpr::And(vec![
                QueryExpr::Text("word1".to_string()),
                QueryExpr::And(vec![
                    QueryExpr::Or(vec![
                        QueryExpr::Text("word2".to_string()),
                        QueryExpr::Text("word3".to_string()),
                    ]),
                    QueryExpr::Text("word4".to_string()),
                ]),
            ])
        );
    }

    #[test]
    fn test_quoted_string() {
        let expr = parse_query("\"hello world\" test").unwrap();
        assert_eq!(
            expr,
            QueryExpr::And(vec![
                QueryExpr::Text("hello world".to_string()),
                QueryExpr::Text("test".to_string()),
            ])
        );
    }

    #[test]
    fn test_tag_with_slash() {
        let expr = parse_query("#project/alpha").unwrap();
        assert_eq!(expr, QueryExpr::Tag("project/alpha".to_string()));
    }

    #[test]
    fn test_link_with_spaces() {
        let expr = parse_query("[[My Great Note]]").unwrap();
        assert_eq!(expr, QueryExpr::Link("My Great Note".to_string()));
    }

    #[test]
    fn test_empty_tag_error() {
        let result = parse_query("#");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Empty tag"));
    }

    #[test]
    fn test_empty_link_error() {
        let result = parse_query("[[]]");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Empty link"));
    }

    #[test]
    fn test_unclosed_link_error() {
        let result = parse_query("[[note");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unclosed"));
    }

    #[test]
    fn test_unclosed_parens_error() {
        let result = parse_query("(word1 OR word2");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unclosed"));
    }

    #[test]
    fn test_unclosed_quote_error() {
        let result = parse_query("\"hello world");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unclosed quote"));
    }

    #[test]
    fn test_or_at_top_level() {
        let expr = parse_query("word1 OR word2").unwrap();
        assert_eq!(
            expr,
            QueryExpr::Or(vec![
                QueryExpr::Text("word1".to_string()),
                QueryExpr::Text("word2".to_string()),
            ])
        );
    }

    #[test]
    fn test_mixed_or_and() {
        let expr = parse_query("word1 word2 OR word3").unwrap();
        // word1 word2 is And, then OR word3
        // So: Or([And([word1, word2]), word3])
        assert_eq!(
            expr,
            QueryExpr::Or(vec![
                QueryExpr::And(vec![
                    QueryExpr::Text("word1".to_string()),
                    QueryExpr::Text("word2".to_string()),
                ]),
                QueryExpr::Text("word3".to_string()),
            ])
        );
    }

    #[test]
    fn test_display_text() {
        let expr = QueryExpr::Text("hello".to_string());
        assert_eq!(expr.to_string(), "hello");
    }

    #[test]
    fn test_display_link() {
        let expr = QueryExpr::Link("note1".to_string());
        assert_eq!(expr.to_string(), "[[note1]]");
    }

    #[test]
    fn test_display_tag() {
        let expr = QueryExpr::Tag("tag1".to_string());
        assert_eq!(expr.to_string(), "#tag1");
    }

    #[test]
    fn test_display_and() {
        let expr = QueryExpr::And(vec![
            QueryExpr::Text("a".to_string()),
            QueryExpr::Text("b".to_string()),
        ]);
        assert_eq!(expr.to_string(), "(a b)");
    }

    #[test]
    fn test_display_or() {
        let expr = QueryExpr::Or(vec![
            QueryExpr::Text("a".to_string()),
            QueryExpr::Text("b".to_string()),
        ]);
        assert_eq!(expr.to_string(), "(a OR b)");
    }

    #[test]
    fn test_at_name_link() {
        let expr = parse_query("@note1").unwrap();
        assert_eq!(expr, QueryExpr::Link("note1".to_string()));
    }

    #[test]
    fn test_at_name_with_underscore() {
        let expr = parse_query("@my_note").unwrap();
        assert_eq!(expr, QueryExpr::Link("my_note".to_string()));
    }

    #[test]
    fn test_at_name_mixed_with_other_terms() {
        let expr = parse_query("word1 @note1 #tag1").unwrap();
        assert_eq!(
            expr,
            QueryExpr::And(vec![
                QueryExpr::Text("word1".to_string()),
                QueryExpr::Link("note1".to_string()),
                QueryExpr::Tag("tag1".to_string()),
            ])
        );
    }

    #[test]
    fn test_at_name_equivalent_to_wiki_link() {
        // @name and [[name]] should produce the same AST
        let at_expr = parse_query("@note1").unwrap();
        let wiki_expr = parse_query("[[note1]]").unwrap();
        assert_eq!(at_expr, wiki_expr);
    }

    #[test]
    fn test_empty_at_error() {
        let result = parse_query("@");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Empty name after @"));
    }

    #[test]
    fn test_attribute_exists() {
        let expr = parse_query("[status]").unwrap();
        assert_eq!(
            expr,
            QueryExpr::Attribute {
                key: "status".to_string(),
                value: None,
            }
        );
    }

    #[test]
    fn test_attribute_with_value() {
        let expr = parse_query("[type:meeting]").unwrap();
        assert_eq!(
            expr,
            QueryExpr::Attribute {
                key: "type".to_string(),
                value: Some("meeting".to_string()),
            }
        );
    }

    #[test]
    fn test_attribute_mixed_with_other_terms() {
        let expr = parse_query("word1 [status] #tag1 [[note1]]").unwrap();
        assert_eq!(
            expr,
            QueryExpr::And(vec![
                QueryExpr::Text("word1".to_string()),
                QueryExpr::Attribute {
                    key: "status".to_string(),
                    value: None,
                },
                QueryExpr::Tag("tag1".to_string()),
                QueryExpr::Link("note1".to_string()),
            ])
        );
    }

    #[test]
    fn test_attribute_with_value_mixed() {
        let expr = parse_query("[author:John] #draft").unwrap();
        assert_eq!(
            expr,
            QueryExpr::And(vec![
                QueryExpr::Attribute {
                    key: "author".to_string(),
                    value: Some("John".to_string()),
                },
                QueryExpr::Tag("draft".to_string()),
            ])
        );
    }

    #[test]
    fn test_attribute_in_or() {
        let expr = parse_query("([status] OR [type:meeting])").unwrap();
        assert_eq!(
            expr,
            QueryExpr::Or(vec![
                QueryExpr::Attribute {
                    key: "status".to_string(),
                    value: None,
                },
                QueryExpr::Attribute {
                    key: "type".to_string(),
                    value: Some("meeting".to_string()),
                },
            ])
        );
    }

    #[test]
    fn test_empty_brackets_error() {
        let result = parse_query("[]");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Empty attribute"));
    }

    #[test]
    fn test_unclosed_bracket_error() {
        let result = parse_query("[status");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unclosed"));
    }

    #[test]
    fn test_display_attribute_exists() {
        let expr = QueryExpr::Attribute {
            key: "status".to_string(),
            value: None,
        };
        assert_eq!(expr.to_string(), "[status]");
    }

    #[test]
    fn test_display_attribute_with_value() {
        let expr = QueryExpr::Attribute {
            key: "type".to_string(),
            value: Some("meeting".to_string()),
        };
        assert_eq!(expr.to_string(), "[type:meeting]");
    }

    #[test]
    fn test_empty_attribute_key_error() {
        let result = parse_query("[:value]");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Empty attribute key"));
    }
}
