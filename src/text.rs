//! Small text helpers shared by `api` and `site`.

/// A short human-readable extract of a response body: a JSON `error` /
/// `message` / `detail`, or the visible text of an HTML page, capped at
/// 300 chars. Control characters are collapsed to spaces so a summary
/// cannot forge log lines.
pub(crate) fn summarize_response(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "(empty response body)".into();
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        for key in ["error", "message", "detail"] {
            match v.get(key) {
                Some(serde_json::Value::String(m)) => {
                    return truncate(&collapse_whitespace(m), 300)
                }
                // `{"error": {"message": "…"}}`
                Some(serde_json::Value::Object(o)) => {
                    for inner in ["message", "detail", "error"] {
                        if let Some(m) = o.get(inner).and_then(|x| x.as_str()) {
                            return truncate(&collapse_whitespace(m), 300);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Tags become spaces so `</p><br/>x` does not glue words together.
    let mut out = String::new();
    let mut in_tag = false;
    for ch in trimmed.chars() {
        match ch {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    truncate(&collapse_whitespace(&out), 300)
}

/// Runs of whitespace and control characters become one space.
pub(crate) fn collapse_whitespace(s: &str) -> String {
    s.split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_strips_html_and_truncates() {
        assert_eq!(summarize_response("  "), "(empty response body)");
        assert_eq!(
            summarize_response("<p>Ошибка   импорта</p><br/>x"),
            "Ошибка импорта x"
        );
        assert_eq!(summarize_response(r#"{"detail":"Not Found"}"#), "Not Found");
        assert_eq!(
            summarize_response(r#"{"error":{"message":"nested"}}"#),
            "nested"
        );
        assert_eq!(
            summarize_response(r#"{"error":"a\r\nb\u001b[31mc"}"#),
            "a b [31mc"
        );
        let long = "я".repeat(400);
        assert_eq!(summarize_response(&long).chars().count(), 301);
    }
}
