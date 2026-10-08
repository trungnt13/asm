"""Lossless instruction pages whose receipt is proven by successful Claude tool hooks."""

import hashlib
import json


class Instructions:
    def initialize_instructions(self, pages):
        if not isinstance(pages, list) or any(
            not isinstance(page, str) for page in pages
        ):
            raise ValueError("Claude instructions must be immutable text pages")
        sizes = [len(page.encode()) for page in pages]
        if any(size > 8000 for size in sizes) or sum(sizes) > 64 * 1024:
            raise ValueError(
                "Claude instruction pages exceed 8000-byte/page or 64-KiB aggregate bounds"
            )
        self.instruction_pages = tuple(pages)
        encoded = json.dumps(pages, ensure_ascii=False, separators=(",", ":")).encode()
        self.instruction_revision = hashlib.sha256(encoded).hexdigest()
        self.instructions_fetched = set()
        self.pending_instruction_prompts = {}
        self.served_instructions = {}
        self.instruction_prompt = None
        self.initial_acknowledged = False

    def read_instruction_page(self, page):
        if type(page) is not int or not 0 <= page < len(self.instruction_pages):
            raise ValueError("Instruction page index is outside the immutable revision")
        candidate = self.pending_instruction_prompts.pop(page, None)
        if not self.active or not candidate:
            raise ValueError("Instruction read lacks an active-task PreToolUse prompt")
        prompt, tool_id = candidate
        if not self.initial_acknowledged and self.instruction_prompt != prompt:
            self.instructions_fetched.clear()
            self.served_instructions.clear()
            self.instruction_prompt = prompt
        header = f"ASM instructions revision {self.instruction_revision}, page {page + 1}/{len(self.instruction_pages)}.\n"
        if len(header.encode()) > 192:
            raise ValueError("Claude instruction page header exceeds 192 bytes")
        text = header + self.instruction_pages[page]
        self.served_instructions[tool_id] = (prompt, page, text)
        return {"text": text}

    def observe_instruction_read(self, payload):
        name = payload["hook_event_name"]
        prompt = payload.get("prompt_id")
        tool_id = payload.get("tool_use_id")
        page = payload.get("tool_input", {}).get("page")
        if name == "PreToolUse":
            if (
                self.active
                and type(page) is int
                and 0 <= page < len(self.instruction_pages)
                and prompt
                and tool_id
            ):
                self.pending_instruction_prompts[page] = (prompt, tool_id)
            return
        if name not in ("PostToolUse", "PostToolUseFailure"):
            return
        served = self.served_instructions.pop(tool_id, None)
        if self.pending_instruction_prompts.get(page) == (prompt, tool_id):
            self.pending_instruction_prompts.pop(page)
        if not served or served[0] != prompt or served[1] != page or not self.active:
            return
        if not self.initial_acknowledged and self.instruction_prompt != prompt:
            return
        response = payload.get("tool_response")
        failed = (
            name == "PostToolUseFailure"
            or isinstance(response, dict)
            and response.get("isError") is True
        )
        if isinstance(response, dict) and "content" in response:
            blocks = response["content"]
            failed |= (
                not isinstance(blocks, list)
                or len(blocks) != 1
                or not isinstance(blocks[0], dict)
                or blocks[0].get("text") != served[2]
            )
        if isinstance(response, str) and response.startswith(
            "ASM instructions revision "
        ):
            failed |= response != served[2]
        if failed:
            self.instructions_fetched.discard(page)
        else:
            self.instructions_fetched.add(page)
