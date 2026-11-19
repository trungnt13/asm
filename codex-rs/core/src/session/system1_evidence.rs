use super::input_queue::TurnInput;
use codex_history::ResponseItemEnvelope;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ImageReference;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::is_local_audio_close_tag_text;
use codex_protocol::models::is_local_audio_open_tag_text;
use codex_protocol::models::is_local_image_close_tag_text;
use codex_protocol::models::is_local_image_open_tag_text;
use codex_user_input::ResolvedUserInput;
use codex_utils_image::PromptImageDecoder;
use codex_utils_image::PromptImageMode;
use codex_utils_image::PromptImageResizeLimits;
use codex_utils_image::data_url_from_bytes;
use codex_utils_image::load_data_url_for_prompt_uncached;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::OnceLock;

const MAX_IMAGE_DATA_URL_BYTES: usize = 4 * 1024 * 1024;
const MAX_LOCAL_IMAGE_BYTES: usize =
    (MAX_IMAGE_DATA_URL_BYTES - "data:image/jpeg;base64,".len()) / 4 * 3;

pub(super) struct System1Image {
    source: ImageSource,
    // The connection is fixed for the session, including its target image dimensions.
    prepared: OnceLock<Option<String>>,
}

enum ImageSource {
    Inline(Arc<ImageReference>),
    Local {
        mime: &'static str,
        bytes: Arc<[u8]>,
    },
}

impl System1Image {
    pub(super) fn prepare(&self, minimum_side: u32) -> Option<String> {
        self.prepared
            .get_or_init(|| self.prepare_uncached(minimum_side))
            .clone()
    }

    fn prepare_uncached(&self, minimum_side: u32) -> Option<String> {
        let source = match &self.source {
            ImageSource::Inline(image) => {
                let ImageReference::Inline { image_url } = image.as_ref() else {
                    unreachable!("System1 only captures inline image references");
                };
                image_url.clone()
            }
            ImageSource::Local { mime, bytes } => data_url_from_bytes(mime, bytes),
        };
        let image = load_data_url_for_prompt_uncached(
            &source,
            PromptImageMode::ResizeWithLimits(PromptImageResizeLimits {
                max_dimension: 1568,
                max_patches: 1024,
            }),
            PromptImageDecoder::Sanitizer,
        )
        .ok()?;
        let url = if image.width < minimum_side || image.height < minimum_side {
            // Pad without distorting the subject. Only decode the already bounded, sanitized copy.
            let mut canvas = image::DynamicImage::new_rgb8(
                image.width.max(minimum_side),
                image.height.max(minimum_side),
            );
            let pixels = image::load_from_memory(&image.bytes).ok()?;
            let x = i64::from((canvas.width() - image.width) / 2);
            let y = i64::from((canvas.height() - image.height) / 2);
            image::imageops::overlay(&mut canvas, &pixels, x, y);
            let mut png = Cursor::new(Vec::new());
            canvas.write_to(&mut png, image::ImageFormat::Png).ok()?;
            data_url_from_bytes("image/png", png.get_ref())
        } else {
            image.into_data_url()
        };
        (url.len() <= MAX_IMAGE_DATA_URL_BYTES).then_some(url)
    }
}

#[derive(Default)]
pub(super) struct System1Evidence {
    pub(super) task: String,
    pub(super) encrypted_task_unavailable: bool,
    pub(super) image: Option<Arc<System1Image>>,
    steer: String,
}

impl System1Evidence {
    pub(super) fn ingest<'a>(
        &mut self,
        input: impl IntoIterator<Item = &'a TurnInput>,
        limit: usize,
        include_images: bool,
    ) {
        // Keep scan work bounded, but let larger byte budgets reach more history and parts.
        let item_limit = limit.div_ceil(/*rhs*/ 64).max(/*other*/ 128);
        let part_limit = limit.div_ceil(/*rhs*/ 256).max(/*other*/ 32);
        let mut incoming = String::new();
        let mut encrypted_input = false;
        let mut image = None;
        for item in input.into_iter().take(item_limit) {
            match item {
                TurnInput::UserInput {
                    content, metadata, ..
                } => {
                    for item in content.iter().take(part_limit) {
                        if let ResolvedUserInput::Text { text, .. } = item {
                            append_text(&mut incoming, text, limit / 3);
                        } else if include_images && image.is_none() && metadata.origin.is_user() {
                            image = decision_image(item);
                        }
                    }
                }
                TurnInput::InterAgentCommunication(communication) => {
                    // Assigned tasks and steering are untrusted text, not message metadata.
                    if communication.encrypted_content.is_none() {
                        append_text(&mut incoming, &communication.content, limit / 3);
                    } else {
                        encrypted_input = true;
                        self.encrypted_task_unavailable = true;
                    }
                }
                TurnInput::FunctionCallOutput(_) | TurnInput::ResponseItem(_) => {}
            }
        }
        if (!incoming.is_empty() || image.is_some()) && !encrypted_input {
            self.encrypted_task_unavailable = false;
        }
        if image.is_some() {
            self.image = image.map(|source| {
                Arc::new(System1Image {
                    source,
                    prepared: OnceLock::new(),
                })
            });
        }
        if !incoming.is_empty() {
            if self.task.is_empty() {
                self.task = incoming;
            } else {
                self.steer = if incoming == self.task {
                    String::new()
                } else {
                    incoming
                };
            }
        }
    }

    pub(super) fn render(&self, history: &[ResponseItemEnvelope], limit: usize) -> String {
        let item_limit = limit.div_ceil(/*rhs*/ 64).max(/*other*/ 128);
        let part_limit = limit.div_ceil(/*rhs*/ 256).max(/*other*/ 32);
        let mut text = String::new();
        append_text(&mut text, "Current user task:\n", limit);
        append_text(
            &mut text,
            if self.task.trim().is_empty() {
                "[Attached image; no text task provided.]"
            } else {
                &self.task
            },
            limit,
        );
        if !self.steer.is_empty() {
            append_text(&mut text, "\nLatest user steer:\n", limit);
            append_text(&mut text, &self.steer, limit * 2 / 3);
        }
        append_text(&mut text, "\nRecent evidence (newest first):\n", limit);
        for envelope in history.iter().rev().take(item_limit) {
            if text.len() >= limit {
                break;
            }
            if !envelope
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.compaction_output)
            {
                append_item(&mut text, envelope, limit, part_limit);
            }
        }
        text
    }
}

fn decision_image(input: &ResolvedUserInput) -> Option<ImageSource> {
    match input {
        ResolvedUserInput::Image { image, .. } => {
            let ImageReference::Inline { image_url } = image.as_ref() else {
                return None;
            };
            (image_url.len() <= MAX_IMAGE_DATA_URL_BYTES
                && (image_url.starts_with("data:image/png;base64,iVBORw0KGgo")
                    || image_url.starts_with("data:image/jpeg;base64,/9j/")))
            .then(|| ImageSource::Inline(Arc::clone(image)))
        }
        ResolvedUserInput::LocalImage {
            bytes: Ok(bytes), ..
        } if bytes.len() <= MAX_LOCAL_IMAGE_BYTES => {
            let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                "image/png"
            } else if bytes.starts_with(b"\xff\xd8\xff") {
                "image/jpeg"
            } else {
                return None;
            };
            Some(ImageSource::Local {
                mime,
                bytes: Arc::clone(bytes),
            })
        }
        _ => None,
    }
}

fn append_text(output: &mut String, text: &str, limit: usize) {
    let remaining = limit.saturating_sub(output.len());
    output.push_str(&text[..text.floor_char_boundary(remaining.min(text.len()))]);
}

fn append_item(
    output: &mut String,
    envelope: &ResponseItemEnvelope,
    limit: usize,
    part_limit: usize,
) {
    match envelope.item.as_ref() {
        ResponseItem::Message { role, content, .. }
            if role == "assistant"
                || (role == "user"
                    && envelope
                        .metadata
                        .as_ref()
                        .is_some_and(|metadata| metadata.user_input_order.is_some())) =>
        {
            append_text(output, &format!("\n{role}: "), limit);
            for content in content.iter().take(part_limit) {
                if output.len() >= limit {
                    break;
                }
                if let ContentItem::InputText { text } | ContentItem::OutputText { text, .. } =
                    content
                {
                    // The user role also carries synthesized local media labels and read errors.
                    if role == "user"
                        && (is_local_image_open_tag_text(text)
                            || is_local_image_close_tag_text(text)
                            || is_local_audio_open_tag_text(text)
                            || is_local_audio_close_tag_text(text)
                            || text.starts_with("Codex could not read the local image at `")
                            || text.starts_with("Codex could not read the local audio at `")
                            || text.starts_with("Codex cannot attach audio at `"))
                    {
                        continue;
                    }
                    append_text(output, text, limit);
                }
            }
        }
        ResponseItem::FunctionCallOutput {
            output: payload, ..
        }
        | ResponseItem::CustomToolCallOutput {
            output: payload, ..
        } => {
            append_text(output, "\ntool: ", limit);
            match &payload.body {
                FunctionCallOutputBody::Text(text) => append_text(output, text, limit),
                FunctionCallOutputBody::ContentItems(items) => {
                    for item in items.iter().take(part_limit) {
                        if output.len() >= limit {
                            break;
                        }
                        if let FunctionCallOutputContentItem::InputText { text } = item {
                            append_text(output, text, limit);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "system1_evidence_tests.rs"]
mod tests;
