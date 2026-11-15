//! `{page}` / `{pages}` tokens in headers, footers and page numbers.

pub fn substitute_page_tokens(format: &str, page: &str, pages: &str) -> String {
    let mut out = String::new();
    let mut rest = format;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if let Some(stripped) = tail.strip_prefix("{{") {
            out.push('{');
            rest = stripped;
        } else if let Some(stripped) = tail.strip_prefix("}}") {
            out.push('}');
            rest = stripped;
        } else if let Some(stripped) = tail.strip_prefix("{page}") {
            out.push_str(page);
            rest = stripped;
        } else if let Some(stripped) = tail.strip_prefix("{pages}") {
            out.push_str(pages);
            rest = stripped;
        } else {
            out.push_str(&tail[..1]);
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

pub fn has_page_tokens(format: &str) -> bool {
    substitute_page_tokens(format, "\u{1}", "\u{1}").contains('\u{1}')
}

pub fn fill_page_tokens(format: &str, page: u32, pages: u32) -> String {
    substitute_page_tokens(format, &page.to_string(), &pages.to_string())
}
