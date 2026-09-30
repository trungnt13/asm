pub(crate) fn compact_model_name(name: &str) -> String {
    let name = name.to_lowercase().replace('.', "");
    let name = name.strip_prefix("gpt-").unwrap_or(&name);
    let (version, family) = name.split_once(['-', ' ']).unwrap_or((name, ""));
    if !version.is_empty() && version.chars().all(|ch| ch.is_ascii_digit()) {
        format!("{family}{version}")
    } else {
        name.to_string()
    }
}

pub(crate) fn compact_reasoning_label(label: &str) -> String {
    label.to_lowercase().chars().take(3).collect()
}
