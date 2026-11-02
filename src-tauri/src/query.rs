//! A small Gmail-like query language used by search and split inboxes.
//!
//! ```text
//! from:alice to:bob subject:invoice account:school is:unread is:starred
//! has:attachment category:newsletter -category:primary in:inbox "exact phrase" words
//! ```
//!
//! Every term compiles to a condition on the `threads t` row; free text goes
//! through the FTS5 index.

use rusqlite::types::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Term {
    From(String),
    To(String),
    Subject(String),
    Account(String),
    Is(String),
    Has(String),
    In(String),
    Category(String),
    Text(String),
    Not(Box<Term>),
}

pub fn parse(q: &str) -> Vec<Term> {
    let mut terms = Vec::new();
    for token in tokenize(q) {
        let (neg, tok) = match token.strip_prefix('-') {
            Some(rest) if !rest.is_empty() => (true, rest.to_string()),
            _ => (false, token),
        };
        let term = match tok.split_once(':') {
            Some((key, val)) if !val.is_empty() => {
                let val = val.trim_matches('"').to_string();
                match key.to_ascii_lowercase().as_str() {
                    "from" => Term::From(val),
                    "to" | "cc" => Term::To(val),
                    "subject" => Term::Subject(val),
                    "account" => Term::Account(val),
                    "is" => Term::Is(val.to_ascii_lowercase()),
                    "has" => Term::Has(val.to_ascii_lowercase()),
                    "in" => Term::In(val.to_ascii_lowercase()),
                    "category" | "cat" => Term::Category(val.to_ascii_lowercase()),
                    _ => Term::Text(tok.trim_matches('"').to_string()),
                }
            }
            _ => Term::Text(tok.trim_matches('"').to_string()),
        };
        terms.push(if neg { Term::Not(Box::new(term)) } else { term });
    }
    terms
}

/// Splits on whitespace while keeping `"quoted phrases"` (also after `key:`) together.
fn tokenize(q: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for ch in q.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                cur.push(ch);
            }
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Builds an FTS5 MATCH expression with prefix matching on every word.
fn fts_expr(text: &str) -> Option<String> {
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric() && c != '@' && c != '.' && c != '-' && c != '_')
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{}\"*", w.replace('"', "")))
        .collect();
    if words.is_empty() { None } else { Some(words.join(" ")) }
}

fn like(v: &str) -> Value {
    format!("%{}%", v.to_lowercase()).into()
}

fn term_sql(term: &Term, vals: &mut Vec<Value>) -> Option<String> {
    let exists = |cond: &str| {
        format!("EXISTS (SELECT 1 FROM messages m WHERE m.thread_id = t.id AND {cond})")
    };
    Some(match term {
        Term::From(v) => {
            vals.push(like(v));
            vals.push(like(v));
            exists("(lower(m.from_email) LIKE ? OR lower(m.from_name) LIKE ?)")
        }
        Term::To(v) => {
            vals.push(like(v));
            exists("lower(m.to_text) LIKE ?")
        }
        Term::Subject(v) => {
            vals.push(like(v));
            "lower(t.subject) LIKE ?".into()
        }
        Term::Account(v) => {
            vals.push(like(v));
            vals.push(like(v));
            vals.push(v.to_lowercase().into());
            "t.account_id IN (SELECT id FROM accounts WHERE lower(email) LIKE ? OR lower(COALESCE(label, '')) LIKE ? OR kind = ?)".into()
        }
        Term::Is(v) => match v.as_str() {
            "unread" => "t.unread > 0".into(),
            "read" => "t.unread = 0".into(),
            "starred" | "flagged" => "t.starred = 1".into(),
            "snoozed" => "tm.snoozed_until IS NOT NULL".into(),
            _ => return None,
        },
        Term::Has(v) => match v.as_str() {
            "attachment" | "attachments" | "file" => "t.has_attachments = 1".into(),
            _ => return None,
        },
        Term::In(v) => match v.as_str() {
            "inbox" => "t.in_inbox = 1".into(),
            "sent" => "t.in_sent = 1".into(),
            "drafts" => "t.in_drafts = 1".into(),
            "trash" => "t.in_trash = 1".into(),
            "spam" => "t.in_spam = 1".into(),
            "done" | "archive" => "t.in_inbox = 0".into(),
            _ => return None,
        },
        Term::Category(v) => {
            let v = match v.as_str() {
                "important" | "personal" => "primary",
                "news" | "newsletters" | "promotions" => "newsletter",
                "notifications" | "updates" => "notification",
                other => other,
            };
            vals.push(v.to_string().into());
            "t.category = ?".into()
        }
        Term::Text(v) => {
            let expr = fts_expr(v)?;
            vals.push(expr.into());
            "t.id IN (SELECT m.thread_id FROM messages_fts f JOIN messages m ON m.rowid = f.rowid WHERE messages_fts MATCH ?)".into()
        }
        Term::Not(inner) => {
            let s = term_sql(inner, vals)?;
            format!("NOT ({s})")
        }
    })
}

pub fn to_sql(terms: &[Term]) -> (String, Vec<Value>) {
    let mut vals = Vec::new();
    let parts: Vec<String> = terms.iter().filter_map(|t| term_sql(t, &mut vals)).collect();
    (parts.join(" AND "), vals)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mixed_query() {
        let t = parse(r#"from:alice -category:primary "hello world" is:unread"#);
        assert_eq!(
            t,
            vec![
                Term::From("alice".into()),
                Term::Not(Box::new(Term::Category("primary".into()))),
                Term::Text("hello world".into()),
                Term::Is("unread".into()),
            ]
        );
    }

    #[test]
    fn fts_escapes_words() {
        assert_eq!(fts_expr("foo \"bar"), Some("\"foo\"* \"bar\"*".into()));
        assert_eq!(fts_expr("  "), None);
    }
}
