pub(crate) fn compact_model_name(name: &str) -> String {
    let name = name.to_lowercase();
    let name = name.strip_prefix("gpt-").unwrap_or(&name);
    let family: String = name
        .chars()
        .filter(|ch| ch.is_alphabetic())
        .take(3)
        .collect();
    let version: String = name.chars().filter(char::is_ascii_digit).collect();
    format!("{family}{version}")
}

pub(crate) fn compact_reasoning_label(label: &str) -> String {
    let (effort, adaptive) = label
        .strip_suffix("·auto")
        .map_or((label, false), |effort| (effort, true));
    let effort: String = effort.to_lowercase().chars().take(3).collect();
    if adaptive {
        format!("{effort}·auto")
    } else {
        effort
    }
}
