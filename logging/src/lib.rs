//! Reusable tracing-subscriber log formatter.
//!
//! Flattens all span fields into a single `{key=val, …}` block, deduplicates
//! identical values, disambiguates conflicts with `span_name.field`, and
//! auto-detects ANSI color support.
//!
//! When an event records a `&dyn Error` field (via tracing's native error
//! visitor), the formatter walks the source chain, deduplicates redundant
//! wrappers, and prints the root as the event message with each remaining
//! cause on its own CAUSE line below.
//!
//! ```text
//! 2026-03-01 10:00:00  INFO creating user account {user=alice, email=a@b.com}
//! 2026-03-01 10:00:00 ERROR Failed to deserialize the JSON body {status=422}
//!                     CAUSE missing field `name` at line 1 column 2
//! 2026-03-01 10:00:00 DEBUG [rustls] Sending warning alert CloseNotify
//! ```

use std::fmt;

use chrono::Utc;
use smallvec::SmallVec;
use tracing::{Event, Level, Subscriber, field};
use tracing_log::NormalizeEvent;
use tracing_subscriber::{
    Layer,
    fmt::{FmtContext, FormatEvent, FormatFields, format::Writer, time::FormatTime},
    registry::LookupSpan,
};

/// Writes `YYYY-MM-DD HH:MM:SS` in UTC
/// no sub-second, no trailing Z, saves on space
struct SecondsTimestamp;

impl FormatTime for SecondsTimestamp {
    fn format_time(&self, w: &mut Writer<'_>) -> fmt::Result {
        write!(w, "{}", Utc::now().format("%Y-%m-%d %H:%M:%S"))
    }
}

// ANSI helpers

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";

fn level_color(level: Level) -> &'static str {
    match level {
        Level::ERROR => "\x1b[31m", // red
        Level::WARN => "\x1b[33m",  // yellow
        Level::INFO => "\x1b[32m",  // green
        Level::DEBUG => "\x1b[34m", // blue
        Level::TRACE => "\x1b[35m", // magenta
    }
}

// -- Span field collection ----------------------------------------------------

/// Bag of `(field_name, formatted_value)` pairs stashed inside each span's
/// extensions so the formatter can read them back without re-visiting.
/// Field names are `&'static str` — tracing metadata is compile-time static.
#[derive(Default, Clone)]
struct SpanFieldStorage(Vec<(&'static str, String)>);

/// Visitor that formats field values into strings for `SpanFieldStorage`.
struct StringVisitor(Vec<(&'static str, String)>);

impl StringVisitor {
    fn new() -> Self {
        Self(Vec::new())
    }
    fn into_inner(self) -> Vec<(&'static str, String)> {
        self.0
    }
}

impl field::Visit for StringVisitor {
    fn record_debug(&mut self, field: &field::Field, value: &dyn fmt::Debug) {
        self.0.push((field.name(), format!("{value:?}")));
    }

    fn record_str(&mut self, field: &field::Field, value: &str) {
        self.0.push((field.name(), value.to_owned()));
    }

    fn record_i64(&mut self, field: &field::Field, value: i64) {
        self.0.push((field.name(), value.to_string()));
    }

    fn record_u64(&mut self, field: &field::Field, value: u64) {
        self.0.push((field.name(), value.to_string()));
    }

    fn record_bool(&mut self, field: &field::Field, value: bool) {
        self.0.push((field.name(), value.to_string()));
    }
}

/// `(headline, optional_causes, structured_fields)` — pulled out to keep
/// clippy happy about the tuple complexity.
type EventParts = (
    String,
    Option<SmallVec<[String; 4]>>,
    Vec<(&'static str, String)>,
);

/// Visitor for events — splits the `message` from structured fields so
/// the message can be printed separately and the fields folded into the
/// `{key=val}` block alongside span fields.
///
/// When a `&dyn Error` is recorded (via [`field::Visit::record_error`])
/// with the field name `"error"`, the visitor walks the source chain,
/// deduplicates wrapper noise, and stores the root + causes separately.
/// Any other error field just gets `Display`'d into the fields block
/// like the default subscriber would.
struct EventVisitor {
    message: Option<String>,
    /// Deduplicated error root from the `"error"` field.
    error_root: Option<String>,
    /// Deduplicated cause chain (everything after root) from the `"error"` field.
    error_causes: SmallVec<[String; 4]>,
    fields: Vec<(&'static str, String)>,
}

impl EventVisitor {
    fn new() -> Self {
        Self {
            message: None,
            error_root: None,
            error_causes: SmallVec::new(),
            fields: Vec::new(),
        }
    }

    /// Resolve the final headline message + optional cause list for the
    /// formatter. If there's both an explicit message AND an error, the
    /// error root joins the cause list so nothing is lost.
    fn into_parts(self) -> EventParts {
        match (self.message, self.error_root) {
            // Error only — root becomes the headline, causes stay as-is
            (None, Some(root)) => {
                let causes = if self.error_causes.is_empty() {
                    None
                } else {
                    Some(self.error_causes)
                };
                (root, causes, self.fields)
            }
            // Both message and error — message is headline, root + causes
            // all become CAUSE lines so nothing from the chain is lost
            (Some(msg), Some(root)) => {
                let mut all_causes: SmallVec<[String; 4]> = SmallVec::new();
                all_causes.push(root);
                all_causes.extend(self.error_causes);
                (msg, Some(all_causes), self.fields)
            }
            // Message only (no error)
            (Some(msg), None) => (msg, None, self.fields),
            // Neither — shouldn't happen but handle gracefully
            (None, None) => (String::new(), None, self.fields),
        }
    }
}

/// Fields injected by the `tracing-log` bridge — we already surface the
/// target as `[crate_name]` via `NormalizeEvent`, so these are just noise.
fn is_log_bridge_field(name: &str) -> bool {
    name.starts_with("log.")
}

impl field::Visit for EventVisitor {
    fn record_error(&mut self, field: &field::Field, value: &(dyn std::error::Error + 'static)) {
        if field.name() == "error" {
            let (root, causes) = build_error_chain(value);
            self.error_root = Some(root);
            self.error_causes = causes;
        } else {
            self.fields.push((field.name(), value.to_string()));
        }
    }

    fn record_debug(&mut self, field: &field::Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}"));
        } else if !is_log_bridge_field(field.name()) {
            self.fields.push((field.name(), format!("{value:?}")));
        }
    }

    fn record_str(&mut self, field: &field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_owned());
        } else if !is_log_bridge_field(field.name()) {
            self.fields.push((field.name(), value.to_owned()));
        }
    }

    fn record_i64(&mut self, field: &field::Field, value: i64) {
        if !is_log_bridge_field(field.name()) {
            self.fields.push((field.name(), value.to_string()));
        }
    }

    fn record_u64(&mut self, field: &field::Field, value: u64) {
        if !is_log_bridge_field(field.name()) {
            self.fields.push((field.name(), value.to_string()));
        }
    }

    fn record_bool(&mut self, field: &field::Field, value: bool) {
        if !is_log_bridge_field(field.name()) {
            self.fields.push((field.name(), value.to_string()));
        }
    }
}

/// Layer that populates [`SpanFieldStorage`] on span creation and updates.
///
/// Must be added to the subscriber registry alongside [`FlattenedFormatter`]
/// for span fields to appear in log output.
pub struct SpanFieldLayer;

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for SpanFieldLayer {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let Some(span) = ctx.span(id) else { return };
        let mut visitor = StringVisitor::new();
        attrs.record(&mut visitor);
        span.extensions_mut()
            .insert(SpanFieldStorage(visitor.into_inner()));
    }

    fn on_record(
        &self,
        id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let Some(span) = ctx.span(id) else { return };
        let mut visitor = StringVisitor::new();
        values.record(&mut visitor);

        let mut extensions = span.extensions_mut();
        if let Some(storage) = extensions.get_mut::<SpanFieldStorage>() {
            for (name, value) in visitor.into_inner() {
                if let Some(existing) = storage.0.iter_mut().find(|(n, _)| *n == name) {
                    existing.1 = value;
                } else {
                    storage.0.push((name, value));
                }
            }
        } else {
            extensions.insert(SpanFieldStorage(visitor.into_inner()));
        }
    }
}

// -- Custom event formatter ---------------------------------------------------

/// Flattens all span fields into one `{key=val, …}` block after the
/// message. Supports ANSI colors (auto-detected via the writer).
///
/// - Duplicate field names with the **same** value are printed once.
/// - Duplicate names with **different** values are disambiguated as
///   `span_name.field_name=value`.
/// - Source target is shown (crate name only) when it doesn't match
///   `target_prefix`.
pub struct FlattenedFormatter {
    /// Events whose target starts with this prefix won't show `[crate]`.
    /// Defaults to `"fckn_gay"`.
    pub target_prefix: &'static str,
}

impl Default for FlattenedFormatter {
    fn default() -> Self {
        Self {
            target_prefix: "fckn_gay",
        }
    }
}

impl<S, N> FormatEvent<S, N> for FlattenedFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let ansi = writer.has_ansi_escapes();
        let level = *event.metadata().level();

        // Timestamp (seconds precision)
        if ansi {
            write!(writer, "{DIM}")?;
        }
        SecondsTimestamp.format_time(&mut writer)?;
        if ansi {
            write!(writer, "{RESET}")?;
        }

        // Level
        if ansi {
            write!(
                writer,
                " {BOLD}{color}{level:>5}{RESET} ",
                color = level_color(level)
            )?;
        } else {
            write!(writer, " {level:>5} ")?;
        }

        // Source target — only for crates that aren't ours, truncated to
        // just the crate name (everything before the first "::").
        // For log-bridged events the raw target is "log", so we recover
        // the real one via NormalizeEvent.
        let normalized = event.normalized_metadata();
        let target = normalized
            .as_ref()
            .map_or_else(|| event.metadata().target(), |m| m.target());
        if !target.starts_with(self.target_prefix) {
            let crate_name = target.split("::").next().unwrap_or(target);
            if ansi {
                write!(writer, "{DIM}[{crate_name}]{RESET} ")?;
            } else {
                write!(writer, "[{crate_name}] ")?;
            }
        }

        // Visit event fields — split message, error chain, and key=val pairs
        let mut ev = EventVisitor::new();
        event.record(&mut ev);

        let (message, causes, ev_fields) = ev.into_parts();

        // Event message (bold if ANSI, quoted otherwise)
        if !message.is_empty() {
            if ansi {
                write!(writer, "{BOLD}{message}{RESET}")?;
            } else {
                write!(writer, "{message}")?;
            }
        }

        // Collect span refs upfront so we can hold all extension guards at
        // once and borrow field values directly — zero String clones.
        let spans: SmallVec<[_; 8]> = ctx
            .event_scope()
            .map(|s| s.from_root().collect())
            .unwrap_or_default();
        let mut guards: SmallVec<[_; 8]> = SmallVec::new();
        for span in &spans {
            guards.push(span.extensions());
        }

        // (span_name, field_name, value) — all borrowed, no cloning
        let mut fields: SmallVec<[(&str, &str, &str); 16]> = SmallVec::new();

        for (span, ext) in spans.iter().zip(guards.iter()) {
            if let Some(storage) = ext.get::<SpanFieldStorage>() {
                for (name, value) in &storage.0 {
                    if !value.is_empty() {
                        fields.push((span.name(), *name, value.as_str()));
                    }
                }
            }
        }

        for (name, value) in &ev_fields {
            if !value.is_empty() {
                fields.push(("", *name, value.as_str()));
            }
        }

        if !fields.is_empty() {
            if ansi {
                write!(writer, " {DIM}{{{RESET}")?;
            } else {
                write!(writer, " {{")?;
            }
            let mut first = true;
            for i in 0..fields.len() {
                let (span_name, name, value) = fields[i];

                let dup_earlier = fields[..i]
                    .iter()
                    .any(|(_, n, v)| *n == name && *v == value);
                if dup_earlier {
                    continue;
                }

                let needs_prefix = fields
                    .iter()
                    .enumerate()
                    .any(|(j, (_, n, v))| j != i && *n == name && *v != value);

                if !first {
                    if ansi {
                        write!(writer, "{DIM},{RESET} ")?;
                    } else {
                        write!(writer, ", ")?;
                    }
                }
                first = false;

                if needs_prefix && !span_name.is_empty() {
                    if ansi {
                        write!(writer, "{DIM}{span_name}.{RESET}{name}{DIM}={RESET}{value}")?;
                    } else {
                        write!(writer, "{span_name}.{name}={value}")?;
                    }
                } else if ansi {
                    write!(writer, "{name}{DIM}={RESET}{value}")?;
                } else {
                    write!(writer, "{name}={value}")?;
                }
            }
            if ansi {
                write!(writer, "{DIM}}}{RESET}")?;
            } else {
                write!(writer, "}}")?;
            }
        }

        // CAUSE lines — each deduplicated cause on its own line, aligned
        // so "CAUSE" sits where the level column is.
        if let Some(ref causes) = causes {
            for cause in causes {
                if ansi {
                    write!(
                        writer,
                        "\n{:>20}{BOLD}{color}CAUSE{RESET} {BOLD}{cause}{RESET}",
                        "",
                        color = level_color(level)
                    )?;
                } else {
                    write!(writer, "\n{:>20}CAUSE {cause}", "")?;
                }
            }
        }

        writeln!(writer)
    }
}

// -- Error chain deduplication -------------------------------------------------

/// Walks a `dyn Error` source chain and deduplicates redundant wrappers.
///
/// Many error types inline `source().to_string()` into their own `Display`
/// after a `: `. We collect every level's Display first, then strip each
/// one's suffix that matches the next level — so each cause appears exactly
/// once.
///
/// Returns `(root_message, deduplicated_causes)`.
fn build_error_chain(err: &dyn std::error::Error) -> (String, SmallVec<[String; 4]>) {
    // Pass 1: collect raw Display strings for every level.
    // Index 0 is the root, rest are causes.
    let mut displays: SmallVec<[String; 5]> = SmallVec::new();
    displays.push(err.to_string());
    let mut cur: &dyn std::error::Error = err;
    while let Some(source) = cur.source() {
        if displays.len() >= 32 {
            break;
        }
        displays.push(source.to_string());
        cur = source;
    }

    // Pass 2: deduplicate. For each level, strip the next level's text
    // (with `: ` separator) from the end. If that fails, try stripping
    // just the next level's text (some errors omit the `: `). Whatever
    // survives is the unique contribution of this level.
    let mut parts: SmallVec<[String; 5]> = SmallVec::new();
    for i in 0..displays.len() {
        let raw = &displays[i];
        let stripped = if i + 1 < displays.len() {
            let next = &displays[i + 1];
            let with_sep = format!(": {next}");
            raw.strip_suffix(with_sep.as_str())
                .or_else(|| raw.strip_suffix(next.as_str()))
                .unwrap_or(raw)
                .trim_end()
                .trim_end_matches(':')
                .trim_end()
        } else {
            raw.as_str()
        };

        // Skip empty entries or exact duplicates of the previous entry
        if stripped.is_empty() {
            continue;
        }
        if let Some(prev) = parts.last()
            && prev == stripped
        {
            continue;
        }
        parts.push(stripped.to_owned());
    }

    // First entry is the root, rest are causes
    let root = if parts.is_empty() {
        err.to_string()
    } else {
        parts.remove(0)
    };
    let mut causes: SmallVec<[String; 4]> = SmallVec::new();
    causes.extend(parts);
    (root, causes)
}

/// A no-op `FormatFields` — [`FlattenedFormatter`] handles all field
/// formatting itself via [`EventVisitor`], so the fmt layer's default
/// field writer must be silenced.
pub struct NullFields;

impl<'writer> FormatFields<'writer> for NullFields {
    fn format_fields<R: tracing_subscriber::field::RecordFields>(
        &self,
        _writer: Writer<'writer>,
        _fields: R,
    ) -> fmt::Result {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper error types for testing build_error_chain

    #[derive(Debug)]
    struct SimpleError(&'static str);

    impl fmt::Display for SimpleError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.0)
        }
    }

    impl std::error::Error for SimpleError {}

    /// Error that includes its source's Display in its own Display (the common
    /// `: source` pattern that build_error_chain is designed to deduplicate).
    #[derive(Debug)]
    struct WrappingError {
        msg: &'static str,
        source: Box<dyn std::error::Error + Send + Sync>,
    }

    impl fmt::Display for WrappingError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}: {}", self.msg, self.source)
        }
    }

    impl std::error::Error for WrappingError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&*self.source)
        }
    }

    /// Error that has a source but does NOT include it in Display.
    #[derive(Debug)]
    struct CleanError {
        msg: &'static str,
        source: Box<dyn std::error::Error + Send + Sync>,
    }

    impl fmt::Display for CleanError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.msg)
        }
    }

    impl std::error::Error for CleanError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&*self.source)
        }
    }

    #[test]
    fn single_error_no_causes() {
        let err = SimpleError("something broke");
        let (root, causes) = build_error_chain(&err);
        assert_eq!(root, "something broke");
        assert!(causes.is_empty());
    }

    #[test]
    fn wrapping_error_deduplicates() {
        // inner Display: "db offline"
        // outer Display: "query failed: db offline"
        // source chain:   outer → inner
        // expected: root="query failed", causes=["db offline"]
        let inner = SimpleError("db offline");
        let outer = WrappingError {
            msg: "query failed",
            source: Box::new(inner),
        };
        let (root, causes) = build_error_chain(&outer);
        assert_eq!(root, "query failed");
        assert_eq!(causes.as_slice(), &["db offline"]);
    }

    #[test]
    fn three_levels_with_dedup() {
        let leaf = SimpleError("connection refused");
        let mid = WrappingError {
            msg: "db error",
            source: Box::new(leaf),
        };
        let top = WrappingError {
            msg: "query failed",
            source: Box::new(mid),
        };
        let (root, causes) = build_error_chain(&top);
        assert_eq!(root, "query failed");
        assert_eq!(causes.as_slice(), &["db error", "connection refused"]);
    }

    #[test]
    fn clean_error_no_display_duplication() {
        // source has a source() but doesn't include it in Display
        let inner = SimpleError("timeout");
        let outer = CleanError {
            msg: "request failed",
            source: Box::new(inner),
        };
        let (root, causes) = build_error_chain(&outer);
        assert_eq!(root, "request failed");
        assert_eq!(causes.as_slice(), &["timeout"]);
    }

    #[test]
    fn pure_wrapper_collapses() {
        // An error whose Display is exactly its source's Display — a pure
        // wrapper that adds nothing. Should collapse to just the inner message.
        let inner = SimpleError("the real error");
        let wrapper = WrappingError {
            msg: "", // Display becomes ": the real error"
            source: Box::new(inner),
        };
        // The wrapper's Display is ": the real error", stripping source
        // leaves just ":" which gets trimmed. So it collapses.
        let (root, causes) = build_error_chain(&wrapper);
        assert_eq!(root, "the real error");
        assert!(causes.is_empty(), "expected no causes, got: {causes:?}");
    }

    #[test]
    fn deeply_nested_chain() {
        // Build a chain deeper than the SmallVec inline capacity
        fn nest(depth: usize) -> Box<dyn std::error::Error + Send + Sync> {
            if depth == 0 {
                Box::new(SimpleError("leaf"))
            } else {
                Box::new(CleanError {
                    msg: Box::leak(format!("level-{depth}").into_boxed_str()),
                    source: nest(depth - 1),
                })
            }
        }
        let err = nest(8);
        let (root, causes) = build_error_chain(&*err);
        assert_eq!(root, "level-8");
        assert_eq!(causes.len(), 8); // level-7 through level-1, plus leaf
        assert_eq!(causes.last().unwrap(), "leaf");
    }
}
