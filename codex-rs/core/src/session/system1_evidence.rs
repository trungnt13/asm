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
use tokio_util::task::AbortOnDropHandle;

const MAX_IMAGE_DATA_URL_BYTES: usize = 4 * 1024 * 1024;
const MAX_LOCAL_IMAGE_BYTES: usize =
    (MAX_IMAGE_DATA_URL_BYTES - "data:image/jpeg;base64,".len()) / 4 * 3;
const MAX_IMAGE_CANDIDATE_BYTES: usize = 2 * MAX_IMAGE_DATA_URL_BYTES;

pub(super) struct System1Image {
    candidates: Vec<ImageCandidate>,
}

struct ImageCandidate {
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
    #[tracing::instrument(skip_all)]
    pub(super) async fn prepare(
        self: &Arc<Self>,
        minimum_side: u32,
    ) -> Result<&[String], tokio::task::JoinError> {
        for (index, candidate) in self.candidates.iter().enumerate() {
            if candidate.prepared.get().is_none() {
                let image = Arc::clone(self);
                AbortOnDropHandle::new(tokio::task::spawn_blocking(move || {
                    let candidate = &image.candidates[index];
                    candidate
                        .prepared
                        .get_or_init(|| candidate.prepare(minimum_side));
                }))
                .await?;
            }
            if let Some(prepared) = candidate.prepared.get().and_then(Option::as_ref) {
                return Ok(std::slice::from_ref(prepared));
            }
        }
        Ok(&[])
    }
}

impl ImageCandidate {
    fn prepare(&self, minimum_side: u32) -> Option<String> {
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

struct EvidenceMessage {
    text: String,
    order: Option<u64>,
    user: bool,
}

#[derive(Default)]
pub(super) struct System1Evidence {
    original: Option<EvidenceMessage>,
    latest_user: Option<EvidenceMessage>,
    agent_assignment: String,
    agent_steer: String,
    pub(super) encrypted_task_unavailable: bool,
    pub(super) image: Option<Arc<System1Image>>,
}

impl System1Evidence {
    pub(super) fn ingest(&mut self, input: &TurnInput, limit: usize, include_images: bool) {
        let mut text = String::new();
        let (order, user) = match input {
            TurnInput::UserInput {
                content, metadata, ..
            } => {
                let mut candidates = Vec::new();
                let mut image_bytes_left = MAX_IMAGE_CANDIDATE_BYTES;
                let mut has_media = false;
                for item in content
                    .iter()
                    .take(limit.div_ceil(/*rhs*/ 256).max(/*other*/ 32))
                {
                    if let ResolvedUserInput::Text { text: part, .. } = item {
                        append_text(&mut text, part, limit / 3);
                    } else if item.is_image() {
                        has_media = true;
                        if include_images
                            && metadata.origin.is_user()
                            && let Some((source, bytes)) = decision_image(item, image_bytes_left)
                        {
                            image_bytes_left -= bytes;
                            candidates.push(ImageCandidate {
                                source,
                                prepared: OnceLock::new(),
                            });
                        }
                    }
                }
                if metadata.origin.is_user() && (has_media || text.trim().is_empty()) {
                    self.image =
                        (!candidates.is_empty()).then(|| Arc::new(System1Image { candidates }));
                }
                (metadata.acceptance_order, metadata.origin.is_user())
            }
            TurnInput::InterAgentCommunication(message) => {
                if message.trigger_turn {
                    if message.encrypted_content.is_some() {
                        self.agent_assignment.clear();
                        self.encrypted_task_unavailable = true;
                    } else {
                        append_text(&mut text, &message.content, limit / 3);
                        if !text.trim().is_empty() {
                            self.agent_assignment = text;
                            self.encrypted_task_unavailable = false;
                        }
                    }
                } else if message.encrypted_content.is_none() {
                    self.agent_steer.clear();
                    append_text(&mut self.agent_steer, &message.content, limit / 3);
                }
                return;
            }
            TurnInput::ResponseItem(_) | TurnInput::FunctionCallOutput(_) => return,
        };
        if user && (!text.trim().is_empty() || self.image.is_some()) {
            self.encrypted_task_unavailable = false;
        }
        let message = EvidenceMessage { text, order, user };
        if self.original.is_none() {
            self.original = Some(message);
        } else if user {
            self.latest_user = Some(message);
        } else {
            self.agent_steer = message.text;
        }
    }

    pub(super) fn has_task_text(&self) -> bool {
        !self.agent_assignment.trim().is_empty()
            || self
                .latest_user
                .as_ref()
                .or(self.original.as_ref())
                .map_or_else(
                    || !self.agent_steer.trim().is_empty(),
                    |m| !m.text.trim().is_empty(),
                )
    }

    pub(super) fn render(&self, history: &[ResponseItemEnvelope], limit: usize) -> String {
        let item_limit = limit.div_ceil(/*rhs*/ 64).max(/*other*/ 128);
        let part_limit = limit.div_ceil(/*rhs*/ 256).max(/*other*/ 32);
        let mut text = String::new();
        let original = self.original.as_ref();
        let latest = self.latest_user.as_ref().or(original.filter(|m| m.user));
        if let Some(message) = latest {
            append_text(&mut text, "Latest user message:\n", limit);
            append_text(
                &mut text,
                if message.text.trim().is_empty() {
                    "[Attachment; no text.]"
                } else {
                    &message.text
                },
                limit,
            );
        }
        if let Some(message) =
            original.filter(|m| latest.is_none_or(|latest| latest.text != m.text))
        {
            if !text.is_empty() {
                append_text(&mut text, "\n", limit);
            }
            append_text(&mut text, "Original task:\n", limit);
            append_text(
                &mut text,
                if message.text.trim().is_empty() {
                    "[Attachment; no text.]"
                } else {
                    &message.text
                },
                limit,
            );
        }
        if !self.agent_assignment.is_empty() {
            if !text.is_empty() {
                append_text(&mut text, "\n", limit);
            }
            append_text(&mut text, "Agent assignment:\n", limit);
            append_text(&mut text, &self.agent_assignment, limit);
        }
        if !self.agent_steer.is_empty() {
            append_text(&mut text, "\nLatest task update:\n", limit);
            append_text(&mut text, &self.agent_steer, limit);
        }
        append_text(&mut text, "\nRecent evidence (newest first):\n", limit);
        for envelope in history.iter().rev().take(item_limit) {
            if text.len() >= limit {
                break;
            }
            if !envelope.metadata.as_ref().is_some_and(|metadata| {
                metadata.compaction_output
                    || metadata.user_input_order.is_some_and(|order| {
                        [original, latest]
                            .into_iter()
                            .flatten()
                            .any(|m| m.order == Some(order))
                    })
            }) {
                let start = text.len();
                append_item(&mut text, envelope, limit, part_limit);
                if let Some(user_text) = text[start..].strip_prefix("\nuser: ")
                    && [original, latest]
                        .into_iter()
                        .flatten()
                        .any(|m| m.text == user_text)
                {
                    text.truncate(start);
                }
            }
        }
        text
    }
}

fn decision_image(input: &ResolvedUserInput, bytes_left: usize) -> Option<(ImageSource, usize)> {
    match input {
        ResolvedUserInput::Image { image, .. } => {
            let ImageReference::Inline { image_url } = image.as_ref() else {
                return None;
            };
            let len = image_url.len();
            (len <= MAX_IMAGE_DATA_URL_BYTES.min(bytes_left)
                && (image_url.starts_with("data:image/png;base64,iVBORw0KGgo")
                    || image_url.starts_with("data:image/jpeg;base64,/9j/")))
            .then(|| (ImageSource::Inline(Arc::clone(image)), len))
        }
        ResolvedUserInput::LocalImage {
            bytes: Ok(bytes), ..
        } if bytes.len() <= MAX_LOCAL_IMAGE_BYTES.min(bytes_left) => {
            let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                "image/png"
            } else if bytes.starts_with(b"\xff\xd8\xff") {
                "image/jpeg"
            } else {
                return None;
            };
            Some((
                ImageSource::Local {
                    mime,
                    bytes: Arc::clone(bytes),
                },
                bytes.len(),
            ))
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
