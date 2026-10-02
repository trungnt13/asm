//! List spacing policies. Transcripts use compact spacing in every rendering phase.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ListSpacing {
    /// Preserve the historical spacing for terminal-owned scrollback.
    AfterMultiline,
    /// Do not add separators between sibling items.
    #[default]
    Compact,
}
