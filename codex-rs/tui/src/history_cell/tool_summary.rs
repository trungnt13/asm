//! Tool identity metadata for optional grouped transcript summaries.

use codex_shell_command::bash::parse_shell_script_into_commands;
use codex_shell_command::parse_command::extract_shell_command;

#[derive(Clone)]
pub(crate) struct ToolCallSummary {
    pub(crate) count: usize,
    /// Shared begin/end previews count once within each displayed group.
    pub(crate) count_key: Option<String>,
    pub(crate) running: bool,
    pub(crate) names: Vec<String>,
}

pub(crate) fn command_names(command: &[String]) -> Vec<String> {
    // Only unwrap scripts the existing parser understands; complex scripts keep the shell name.
    let commands = extract_shell_command(command)
        .and_then(|(_, script)| parse_shell_script_into_commands(script))
        .filter(|commands| !commands.is_empty())
        .unwrap_or_else(|| vec![command.to_vec()]);
    let mut names = Vec::new();
    for command in commands {
        if let Some(executable) = command.first() {
            let name = executable.rsplit(['/', '\\']).next().unwrap_or(executable);
            if !name.is_empty() && !names.iter().any(|existing| existing == name) {
                names.push(name.to_owned());
            }
        }
    }
    names
}
