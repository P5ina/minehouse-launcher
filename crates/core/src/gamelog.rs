//! Turns the game's log4j XML output into readable records.
//!
//! Mojang's logging config (`-Dlog4j.configurationFile=client-*.xml`) makes the
//! client print every log line to stdout as an `XMLLayout` event:
//!
//! ```text
//! <log4j:Event logger="net.minecraft.client.Minecraft" timestamp="1793815501458" level="INFO" thread="Render thread">
//!   <log4j:Message><![CDATA[Connecting to play.minehouse.pro, 25524]]></log4j:Message>
//! </log4j:Event>
//! ```
//!
//! One event spans several lines, so [`Log4jParser`] buffers lines until the
//! closing tag. Anything outside an event (plain `System.out`, JVM warnings)
//! comes back untouched.

use serde::Serialize;

/// One parsed log4j event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GameLine {
    /// Unix time in milliseconds.
    pub time: i64,
    /// TRACE, DEBUG, INFO, WARN, ERROR or FATAL.
    pub level: String,
    pub logger: String,
    pub thread: String,
    /// The message, with a stack trace appended on the next lines if there was one.
    pub message: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    /// A whole event.
    Game(GameLine),
    /// A line that is not part of a log4j event.
    Plain(String),
}

/// Stateful line-by-line parser for one output stream.
#[derive(Default)]
pub struct Log4jParser {
    buf: Option<String>,
}

/// An event that never closes is let go after this many bytes.
const MAX_EVENT: usize = 256 * 1024;

impl Log4jParser {
    /// Feeds one line; returns what became complete with it.
    pub fn push(&mut self, line: &str) -> Vec<Parsed> {
        let mut out = Vec::new();
        match self.buf.take() {
            Some(mut buf) => {
                buf.push('\n');
                buf.push_str(line);
                if line.contains("</log4j:Event>") {
                    out.push(finish(buf));
                } else if buf.len() > MAX_EVENT {
                    out.push(Parsed::Plain(buf));
                } else {
                    self.buf = Some(buf);
                }
            }
            None => {
                if line.trim_start().starts_with("<log4j:Event") {
                    if line.contains("</log4j:Event>") {
                        out.push(finish(line.to_owned()));
                    } else {
                        self.buf = Some(line.to_owned());
                    }
                } else {
                    out.push(Parsed::Plain(line.to_owned()));
                }
            }
        }
        out
    }

    /// Whatever is left when the stream ends.
    pub fn flush(&mut self) -> Option<Parsed> {
        self.buf.take().map(Parsed::Plain)
    }
}

fn finish(xml: String) -> Parsed {
    match parse_event(&xml) {
        Some(l) => Parsed::Game(l),
        None => Parsed::Plain(xml),
    }
}

fn parse_event(xml: &str) -> Option<GameLine> {
    let start = xml.find("<log4j:Event")?;
    let head_end = start + xml[start..].find('>')?;
    let head = &xml[start..head_end];
    let level = attr(head, "level").unwrap_or_else(|| "INFO".into());
    let logger = attr(head, "logger").unwrap_or_default();
    let thread = attr(head, "thread").unwrap_or_default();
    let time = attr(head, "timestamp")
        .and_then(|t| t.parse().ok())
        .unwrap_or(0);
    let body = &xml[head_end + 1..];
    let mut message = element(body, "log4j:Message").unwrap_or_default();
    if let Some(t) = element(body, "log4j:Throwable") {
        let t = t.trim_end();
        if !t.is_empty() {
            if !message.is_empty() {
                message.push('\n');
            }
            message.push_str(t);
        }
    }
    Some(GameLine {
        time,
        level,
        logger,
        thread,
        message,
    })
}

/// Value of `name="..."` in a start tag.
fn attr(head: &str, name: &str) -> Option<String> {
    let mut rest = head;
    loop {
        let i = rest.find(name)?;
        let before = rest[..i].chars().next_back();
        let after = &rest[i + name.len()..];
        if before.is_some_and(char::is_whitespace) {
            if let Some(v) = after.strip_prefix("=\"") {
                let end = v.find('"')?;
                return Some(unescape(&v[..end]));
            }
        }
        rest = after;
    }
}

/// Text of `<tag>...</tag>`, CDATA unwrapped.
fn element(body: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let s = body.find(&open)? + open.len();
    let e = s + body[s..].find(&close)?;
    Some(text(&body[s..e]))
}

/// Joins CDATA sections and unescapes the text between them. log4j splits a
/// message containing `]]>` into several sections.
fn text(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(i) = rest.find("<![CDATA[") {
        out.push_str(&unescape(&rest[..i]));
        let after = &rest[i + 9..];
        match after.find("]]>") {
            Some(e) => {
                out.push_str(&after[..e]);
                rest = &after[e + 3..];
            }
            None => {
                out.push_str(after);
                rest = "";
            }
        }
    }
    out.push_str(&unescape(rest));
    out
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#13;", "\r")
        .replace("&#10;", "\n")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(lines: &[&str]) -> Vec<Parsed> {
        let mut p = Log4jParser::default();
        let mut out: Vec<_> = lines.iter().flat_map(|l| p.push(l)).collect();
        out.extend(p.flush());
        out
    }

    #[test]
    fn parses_multiline_event() {
        let out = feed(&[
            r#"<log4j:Event logger="net.minecraft.client.gui.screens.ConnectScreen" timestamp="1793815502032" level="INFO" thread="Render thread">"#,
            r#"  <log4j:Message><![CDATA[Connecting to play.minehouse.pro, 25524]]></log4j:Message>"#,
            r#"</log4j:Event>"#,
        ]);
        assert_eq!(
            out,
            vec![Parsed::Game(GameLine {
                time: 1793815502032,
                level: "INFO".into(),
                logger: "net.minecraft.client.gui.screens.ConnectScreen".into(),
                thread: "Render thread".into(),
                message: "Connecting to play.minehouse.pro, 25524".into(),
            })]
        );
    }

    #[test]
    fn keeps_plain_lines_and_throwables() {
        let out = feed(&[
            "OpenJDK 64-Bit Server VM warning: something",
            r#"<log4j:Event logger="x" timestamp="1" level="ERROR" thread="main &quot;1&quot;">"#,
            r#"  <log4j:Message><![CDATA[Boom]]>]]&gt;<![CDATA[ tail]]></log4j:Message>"#,
            r#"  <log4j:Throwable><![CDATA[java.lang.RuntimeException: bad"#,
            "\tat a.b.C.d(C.java:1)",
            r#"]]></log4j:Throwable>"#,
            r#"</log4j:Event>"#,
        ]);
        assert_eq!(
            out[0],
            Parsed::Plain("OpenJDK 64-Bit Server VM warning: something".into())
        );
        let Parsed::Game(g) = &out[1] else { panic!() };
        assert_eq!(g.level, "ERROR");
        assert_eq!(g.thread, "main \"1\"");
        assert_eq!(
            g.message,
            "Boom]]> tail\njava.lang.RuntimeException: bad\n\tat a.b.C.d(C.java:1)"
        );
    }

    #[test]
    fn unfinished_event_is_flushed_as_text() {
        let out = feed(&[r#"<log4j:Event logger="x" timestamp="1" level="INFO" thread="t">"#]);
        assert!(matches!(&out[0], Parsed::Plain(s) if s.starts_with("<log4j:Event")));
    }
}
