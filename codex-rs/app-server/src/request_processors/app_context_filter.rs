use codex_core::config::Config;

pub(super) fn apply(config: &mut Config) {
    if config.app_context_filter.is_empty() {
        return;
    }
    let Some(instructions) = config.developer_instructions.as_deref() else {
        return;
    };
    let mut output = String::new();
    let mut copied = 0;
    let mut cursor = 0;
    while let Some(start) = instructions[cursor..].find("<app-context>") {
        let start = cursor + start + "<app-context>".len();
        let Some(end) = instructions[start..].find("</app-context>") else {
            break;
        };
        let end = start + end;
        let body = &instructions[start..end];
        cursor = end + "</app-context>".len();
        if body.trim_start().lines().next() != Some("# Codex desktop context")
            || body.contains("<app-context>")
        {
            continue;
        }
        let mut section_start = None;
        let mut offset = start;
        let mut fence = None;
        for line in body.split_inclusive('\n') {
            let trimmed = line.trim_end_matches(['\r', '\n']);
            let fence_line = trimmed.trim_start_matches(' ');
            if trimmed.len() - fence_line.len() <= 3
                && let Some(marker @ (b'`' | b'~')) = fence_line.bytes().next()
            {
                let length = fence_line
                    .bytes()
                    .take_while(|byte| *byte == marker)
                    .count();
                match fence {
                    Some((open_marker, open_length))
                        if marker == open_marker
                            && length >= open_length
                            && fence_line[length..].trim().is_empty() =>
                    {
                        fence = None;
                    }
                    None if length >= 3 => fence = Some((marker, length)),
                    Some(_) | None => {}
                }
            }
            if fence.is_none()
                && let Some(heading) = trimmed.strip_prefix("### ")
            {
                if let Some(section_start) = section_start.take() {
                    output.push_str(&instructions[copied..section_start]);
                    copied = offset;
                }
                if config
                    .app_context_filter
                    .iter()
                    .any(|section| section == heading)
                {
                    section_start = Some(offset);
                }
            }
            offset += line.len();
        }
        if let Some(section_start) = section_start {
            output.push_str(&instructions[copied..section_start]);
            copied = end;
        }
    }
    if copied != 0 {
        output.push_str(&instructions[copied..]);
        config.developer_instructions = Some(output);
    }
}
