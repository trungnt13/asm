//! Local file-link destination parsing and display for Markdown transcripts.
//!
//! Terminal links use lexical file URLs without filesystem access.
//! Text-only output retains destination text, shortened relative to the session working directory.
//! Never infer execution-host paths from the frontend home or filesystem.

use codex_utils_path_uri::LegacyAppPathString;
use codex_utils_path_uri::PathUri;
use codex_utils_string::normalize_markdown_hash_location_suffix;
use regex_lite::Regex;
use std::path::Path;
use std::sync::LazyLock;
use url::Url;

static COLON_LOCATION_SUFFIX_RE: LazyLock<Regex> =
    LazyLock::new(
        || match Regex::new(r":\d+(?::\d+)?(?:[-–]\d+(?::\d+)?)?$") {
            Ok(regex) => regex,
            Err(error) => panic!("invalid location suffix regex: {error}"),
        },
    );

static HASH_LOCATION_SUFFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| match Regex::new(r"^L\d+(?:C\d+)?(?:-L\d+(?:C\d+)?)?$") {
        Ok(regex) => regex,
        Err(error) => panic!("invalid hash location regex: {error}"),
    });

pub(super) fn is_local_path_like_link(dest_url: &str) -> bool {
    dest_url
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file:"))
        || dest_url.starts_with('/')
        || dest_url.starts_with("~/")
        || dest_url.starts_with("./")
        || dest_url.starts_with("../")
        || dest_url.starts_with("\\\\")
        || matches!(
            dest_url.as_bytes(),
            [drive, b':', separator, ..]
                if drive.is_ascii_alphabetic() && matches!(separator, b'/' | b'\\')
        )
}

pub(super) fn is_markdown_file_link(destination: &str) -> bool {
    is_local_path_like_link(destination)
        || (!destination.is_empty()
            && !destination.starts_with('#')
            && (Url::parse(destination).is_err()
                || extract_colon_location_suffix(destination).is_some_and(|suffix| {
                    let basename = &destination[..destination.len() - suffix.len()];
                    basename.contains('.') && Url::parse(basename).is_err()
                })))
}

// Build URLs lexically: the session can use another OS, home, or filesystem.
pub(super) fn local_link_destination(destination: &str, cwd: Option<&Path>) -> Option<String> {
    if destination.chars().any(char::is_control) || destination.len() > 8 * 1024 {
        return None;
    }
    let (mut path, location) = parse_local_link_target(destination)?;
    if !is_absolute_local_link_path(&path) {
        if path.starts_with("~/") {
            return None;
        }
        let cwd = normalize_local_link_path_text(cwd?.to_str()?);
        if !is_absolute_local_link_path(&cwd) {
            return None;
        }
        path = format!("{}/{path}", cwd.trim_end_matches('/'));
    }
    if path.chars().any(char::is_control) {
        return None;
    }
    let path = LegacyAppPathString::from_string(path).to_inferred_path_uri()?;
    let mut url = Url::parse(&path.to_string()).ok()?;
    // File URL fragments retain the reference, but line opening remains terminal-dependent.
    let fragment = destination
        .rsplit_once('#')
        .map(|(_, fragment)| fragment)
        .filter(|fragment| HASH_LOCATION_SUFFIX_RE.is_match(fragment))
        .or_else(|| {
            location
                .as_deref()
                .map(|location| location.trim_start_matches(':'))
        });
    url.set_fragment(fragment);
    Some(url.to_string())
}

/// Parse a local link target into normalized path text plus an optional location suffix.
///
/// This accepts the path shapes Codex emits today: `file://` URLs, absolute and relative paths,
/// `~/...`, Windows paths, and `#L..C..` or `:line:col` suffixes.
pub(super) fn render_local_link_target(dest_url: &str, cwd: Option<&Path>) -> Option<String> {
    let (path_text, location_suffix) = parse_local_link_target(dest_url)?;
    let mut rendered = display_local_link_path(&path_text, cwd);
    if let Some(location_suffix) = location_suffix {
        rendered.push_str(&location_suffix);
    }
    Some(rendered)
}

/// Split a local-link destination into `(normalized_path_text, location_suffix)`.
///
/// The returned path text never includes a trailing `#L..` or `:line[:col]` suffix. Path
/// normalization preserves `~/...` and rewrites path separators into display-stable forward
/// slashes. The suffix, when present, is returned separately in normalized markdown form.
///
/// Returns `None` only when the destination looks like a `file://` URL but cannot be parsed into a
/// local path. Plain path-like inputs always return `Some(...)` even if they are relative.
fn parse_local_link_target(dest_url: &str) -> Option<(String, Option<String>)> {
    if dest_url
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file:"))
    {
        let url = Url::parse(dest_url).ok()?;
        // Infer the path's OS from the URI, not the host running the TUI.
        let path = PathUri::parse(&url[..url::Position::AfterPath]).ok()?;
        let mut path_text = normalize_local_link_path_text(&path.inferred_native_path_string());
        let mut location_suffix = url
            .fragment()
            .and_then(normalize_hash_location_suffix_fragment);
        if location_suffix.is_none()
            && let Some(suffix) = extract_colon_location_suffix(url.path())
        {
            path_text.truncate(path_text.len() - suffix.len());
            location_suffix = Some(suffix);
        }
        return Some((path_text, location_suffix));
    }

    let mut path_text = dest_url;
    let mut location_suffix = None;
    // Prefer `#L..` style fragments when both forms are present so URLs like `path#L10` do not
    // get misparsed as a plain path ending in `:10`.
    if let Some((candidate_path, fragment)) = dest_url.rsplit_once('#')
        && let Some(normalized) = normalize_hash_location_suffix_fragment(fragment)
    {
        path_text = candidate_path;
        location_suffix = Some(normalized);
    }
    if location_suffix.is_none()
        && let Some(suffix) = extract_colon_location_suffix(path_text)
    {
        let path_len = path_text.len().saturating_sub(suffix.len());
        path_text = &path_text[..path_len];
        location_suffix = Some(suffix);
    }

    let decoded_path_text =
        urlencoding::decode(path_text).unwrap_or(std::borrow::Cow::Borrowed(path_text));
    Some((
        normalize_local_link_path_text(&decoded_path_text),
        location_suffix,
    ))
}

/// Normalize a hash fragment like `L12` or `L12C3-L14C9` into the display suffix we render.
///
/// Returns `None` for fragments that are not location references. This deliberately ignores other
/// `#...` fragments so non-location hashes stay part of the path text.
fn normalize_hash_location_suffix_fragment(fragment: &str) -> Option<String> {
    HASH_LOCATION_SUFFIX_RE
        .is_match(fragment)
        .then(|| format!("#{fragment}"))
        .and_then(|suffix| normalize_markdown_hash_location_suffix(&suffix))
}

/// Extract a trailing `:line`, `:line:col`, or range suffix from a plain path-like string.
///
/// The suffix must occur at the end of the input; embedded colons elsewhere in the path are left
/// alone. This is what keeps Windows drive letters like `C:/...` from being misread as locations.
pub(super) fn extract_colon_location_suffix(path_text: &str) -> Option<String> {
    COLON_LOCATION_SUFFIX_RE
        .find(path_text)
        .filter(|matched| matched.end() == path_text.len())
        .map(|matched| matched.as_str().to_string())
}

/// Normalize local-path text into the transcript display form.
///
/// Display normalization is intentionally lexical: it does not touch the filesystem, resolve
/// symlinks, or collapse `.` / `..`. It only converts separators to forward slashes and rewrites
/// UNC-style `\\\\server\\share` inputs into `//server/share` so later prefix checks operate on a
/// stable representation.
fn normalize_local_link_path_text(path_text: &str) -> String {
    // Render all local link paths with forward slashes so display and prefix stripping are stable
    // across mixed Windows and Unix-style inputs.
    if let Some(rest) = path_text.strip_prefix("\\\\") {
        format!("//{}", rest.replace('\\', "/").trim_start_matches('/'))
    } else {
        path_text.replace('\\', "/")
    }
}

fn is_absolute_local_link_path(path_text: &str) -> bool {
    path_text.starts_with('/')
        || path_text.starts_with("//")
        || matches!(
            path_text.as_bytes(),
            [drive, b':', b'/', ..] if drive.is_ascii_alphabetic()
        )
}

/// Remove trailing separators from a local path without destroying root semantics.
///
/// Roots like `/`, `//`, and `C:/` stay intact so callers can still distinguish "the root itself"
/// from "a path under the root".
fn trim_trailing_local_path_separator(path_text: &str) -> &str {
    if path_text == "/" || path_text == "//" {
        return path_text;
    }
    if matches!(path_text.as_bytes(), [drive, b':', b'/'] if drive.is_ascii_alphabetic()) {
        return path_text;
    }
    path_text.trim_end_matches('/')
}

/// Strip `cwd_text` from the start of `path_text` when `path_text` is strictly underneath it.
///
/// Returns the relative remainder without a leading slash. If the path equals the cwd exactly, this
/// returns `None` so callers can keep rendering the full path instead of collapsing it to an empty
/// string.
fn strip_local_path_prefix<'a>(path_text: &'a str, cwd_text: &str) -> Option<&'a str> {
    let path_text = trim_trailing_local_path_separator(path_text);
    let cwd_text = trim_trailing_local_path_separator(cwd_text);
    if path_text == cwd_text {
        return None;
    }

    // Treat filesystem roots specially so `/tmp/x` under `/` becomes `tmp/x` instead of being
    // left unchanged by the generic prefix-stripping branch.
    if cwd_text == "/" || cwd_text == "//" {
        return path_text.strip_prefix('/');
    }

    path_text
        .strip_prefix(cwd_text)
        .and_then(|rest| rest.strip_prefix('/'))
}

/// Choose the visible path text for a local link after normalization.
///
/// Relative paths (including `~/...`) stay relative. Absolute paths prefer cwd-relative display
/// and otherwise stay absolute: the frontend home may differ from the execution host's home.
/// This is display logic only, not filesystem canonicalization.
fn display_local_link_path(path_text: &str, cwd: Option<&Path>) -> String {
    let path_text = normalize_local_link_path_text(path_text);
    if !is_absolute_local_link_path(&path_text) {
        return path_text;
    }

    if let Some(cwd) = cwd {
        // Only the session cwd is known to refer to the execution host.
        let cwd_text = normalize_local_link_path_text(&cwd.to_string_lossy());
        if let Some(stripped) = strip_local_path_prefix(&path_text, &cwd_text) {
            return stripped.to_string();
        }
    }

    path_text
}

#[cfg(test)]
#[path = "local_links_tests.rs"]
mod tests;
