"""LLM-based metadata extraction for Doomworld forum posts.

This module provides intelligent extraction of WAD metadata from forum posts
using various LLM backends. It's designed to supplement regex-based extraction
with more nuanced understanding of natural language descriptions.

Backend priority (auto-detection):
1. claude-code - Uses local `claude` CLI (cheapest for Claude Code users)
2. openrouter - OpenRouter API (requires OPENROUTER_API_KEY)
3. anthropic - Direct Anthropic API (requires ANTHROPIC_API_KEY)
4. openai - Direct OpenAI API (requires OPENAI_API_KEY)
"""

import json
import os
import shutil
import subprocess
from abc import ABC, abstractmethod
from typing import Any

import httpx


class LLMError(Exception):
    """Error from LLM parsing."""
    pass


class LLMNotAvailableError(LLMError):
    """No LLM backend is available."""
    pass


# =============================================================================
# Extraction Prompt
# =============================================================================

EXTRACTION_PROMPT = '''You are extracting metadata from a Doom WAD release post on the Doomworld forums.

Analyze the following forum post and extract structured metadata. Return a JSON object with these fields:

{{
  "title": "WAD title (if different from thread title)",
  "author": "Author name(s)",
  "description": "Brief 1-2 sentence description of the WAD",
  "iwad": "Required IWAD: doom, doom2, tnt, plutonia, heretic, hexen, or null",
  "sourceport": "Required sourceport: gzdoom, dsda-doom, crispy-doom, eternity, prboom+, etc. or null",
  "complevel": "Compatibility level as integer (2=vanilla, 9=boom, 11=mbf, 21=mbf21) or null",
  "map_count": "Number of maps (integer) or null if unknown",
  "difficulty": "Stated difficulty: easy, medium, hard, slaughter, or null",
  "themes": ["array", "of", "themes/genres"],
  "download_url": "Primary download URL or null",
  "version": "Version string if mentioned (e.g., 'v1.0', 'RC2') or null"
}}

Important:
- Only include information explicitly stated or strongly implied in the post
- Use null for fields where information is not available
- For iwad/sourceport, use lowercase normalized names
- For themes, use terms like: techbase, hell, gothic, city, abstract, puzzle, slaughter, adventure

Forum post to analyze:
---
{post_text}
---

Return ONLY the JSON object, no other text.'''


# =============================================================================
# Parsed Result Model
# =============================================================================

class LLMExtractedMetadata:
    """Metadata extracted by LLM from forum post."""

    def __init__(self, data: dict[str, Any]):
        self.title: str | None = data.get("title")
        self.author: str | None = data.get("author")
        self.description: str | None = data.get("description")
        self.iwad: str | None = data.get("iwad")
        self.sourceport: str | None = data.get("sourceport")
        self.complevel: int | None = data.get("complevel")
        self.map_count: int | None = data.get("map_count")
        self.difficulty: str | None = data.get("difficulty")
        self.themes: list[str] = data.get("themes") or []
        self.download_url: str | None = data.get("download_url")
        self.version: str | None = data.get("version")
        self._raw = data

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary."""
        return {
            "title": self.title,
            "author": self.author,
            "description": self.description,
            "iwad": self.iwad,
            "sourceport": self.sourceport,
            "complevel": self.complevel,
            "map_count": self.map_count,
            "difficulty": self.difficulty,
            "themes": self.themes,
            "download_url": self.download_url,
            "version": self.version,
        }


# =============================================================================
# Abstract Base Class
# =============================================================================

class LLMParser(ABC):
    """Abstract base class for LLM parsing backends."""

    @property
    @abstractmethod
    def name(self) -> str:
        """Human-readable name of the backend."""
        pass

    @abstractmethod
    def parse(self, post_text: str) -> LLMExtractedMetadata:
        """Parse forum post text and extract metadata.

        Args:
            post_text: Plain text content of the forum post

        Returns:
            LLMExtractedMetadata with extracted fields

        Raises:
            LLMError: If parsing fails
        """
        pass

    def _parse_json_response(self, response: str) -> dict[str, Any]:
        """Parse JSON from LLM response, handling markdown code blocks."""
        text = response.strip()

        # Handle markdown code blocks
        if text.startswith("```"):
            lines = text.split("\n")
            # Remove first line (```json or ```)
            lines = lines[1:]
            # Remove last line if it's ```
            if lines and lines[-1].strip() == "```":
                lines = lines[:-1]
            text = "\n".join(lines)

        try:
            return json.loads(text)
        except json.JSONDecodeError as e:
            raise LLMError(f"Failed to parse LLM response as JSON: {e}")


# =============================================================================
# Claude Code Backend (Local CLI)
# =============================================================================

class ClaudeCodeParser(LLMParser):
    """Uses local `claude` CLI program.

    This is the cheapest option for Claude Code users as it uses the
    existing Claude Code subscription rather than API credits.
    """

    @property
    def name(self) -> str:
        return "claude-code"

    def parse(self, post_text: str) -> LLMExtractedMetadata:
        prompt = EXTRACTION_PROMPT.format(post_text=post_text[:8000])  # Limit input size

        try:
            result = subprocess.run(
                ["claude", "--print", "--output-format", "json", "-p", prompt],
                capture_output=True,
                text=True,
                timeout=60,
            )

            if result.returncode != 0:
                raise LLMError(f"Claude CLI failed: {result.stderr}")

            # Parse the JSON output from claude CLI
            try:
                cli_output = json.loads(result.stdout)
                # The actual response is in the 'result' field
                response_text = cli_output.get("result", result.stdout)
            except json.JSONDecodeError:
                # If not JSON, use raw output
                response_text = result.stdout

            data = self._parse_json_response(response_text)
            return LLMExtractedMetadata(data)

        except subprocess.TimeoutExpired:
            raise LLMError("Claude CLI timed out")
        except FileNotFoundError:
            raise LLMError("Claude CLI not found")


# =============================================================================
# OpenRouter Backend
# =============================================================================

class OpenRouterParser(LLMParser):
    """Uses OpenRouter API for access to multiple models."""

    API_URL = "https://openrouter.ai/api/v1/chat/completions"
    DEFAULT_MODEL = "anthropic/claude-3-haiku"

    def __init__(self, model: str | None = None):
        self.model = model or self.DEFAULT_MODEL
        self.api_key = os.environ.get("OPENROUTER_API_KEY")
        if not self.api_key:
            raise LLMNotAvailableError("OPENROUTER_API_KEY not set")

    @property
    def name(self) -> str:
        return f"openrouter ({self.model})"

    def parse(self, post_text: str) -> LLMExtractedMetadata:
        prompt = EXTRACTION_PROMPT.format(post_text=post_text[:8000])

        headers = {
            "Authorization": f"Bearer {self.api_key}",
            "Content-Type": "application/json",
            "HTTP-Referer": "https://github.com/eshen/caco",
            "X-Title": "Caco WAD Library Manager",
        }

        payload = {
            "model": self.model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.1,  # Low temperature for consistent extraction
        }

        try:
            with httpx.Client(timeout=60.0) as client:
                response = client.post(self.API_URL, headers=headers, json=payload)
                response.raise_for_status()

            result = response.json()
            content = result["choices"][0]["message"]["content"]
            data = self._parse_json_response(content)
            return LLMExtractedMetadata(data)

        except httpx.HTTPStatusError as e:
            raise LLMError(f"OpenRouter API error: {e.response.status_code}")
        except (KeyError, IndexError) as e:
            raise LLMError(f"Unexpected OpenRouter response format: {e}")


# =============================================================================
# Anthropic Backend
# =============================================================================

class AnthropicParser(LLMParser):
    """Uses direct Anthropic API."""

    API_URL = "https://api.anthropic.com/v1/messages"
    DEFAULT_MODEL = "claude-3-haiku-20240307"

    def __init__(self, model: str | None = None):
        self.model = model or self.DEFAULT_MODEL
        self.api_key = os.environ.get("ANTHROPIC_API_KEY")
        if not self.api_key:
            raise LLMNotAvailableError("ANTHROPIC_API_KEY not set")

    @property
    def name(self) -> str:
        return f"anthropic ({self.model})"

    def parse(self, post_text: str) -> LLMExtractedMetadata:
        prompt = EXTRACTION_PROMPT.format(post_text=post_text[:8000])

        headers = {
            "x-api-key": self.api_key,
            "Content-Type": "application/json",
            "anthropic-version": "2023-06-01",
        }

        payload = {
            "model": self.model,
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.1,
        }

        try:
            with httpx.Client(timeout=60.0) as client:
                response = client.post(self.API_URL, headers=headers, json=payload)
                response.raise_for_status()

            result = response.json()
            content = result["content"][0]["text"]
            data = self._parse_json_response(content)
            return LLMExtractedMetadata(data)

        except httpx.HTTPStatusError as e:
            raise LLMError(f"Anthropic API error: {e.response.status_code}")
        except (KeyError, IndexError) as e:
            raise LLMError(f"Unexpected Anthropic response format: {e}")


# =============================================================================
# OpenAI Backend
# =============================================================================

class OpenAIParser(LLMParser):
    """Uses direct OpenAI API."""

    API_URL = "https://api.openai.com/v1/chat/completions"
    DEFAULT_MODEL = "gpt-3.5-turbo"

    def __init__(self, model: str | None = None):
        self.model = model or self.DEFAULT_MODEL
        self.api_key = os.environ.get("OPENAI_API_KEY")
        if not self.api_key:
            raise LLMNotAvailableError("OPENAI_API_KEY not set")

    @property
    def name(self) -> str:
        return f"openai ({self.model})"

    def parse(self, post_text: str) -> LLMExtractedMetadata:
        prompt = EXTRACTION_PROMPT.format(post_text=post_text[:8000])

        headers = {
            "Authorization": f"Bearer {self.api_key}",
            "Content-Type": "application/json",
        }

        payload = {
            "model": self.model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.1,
        }

        try:
            with httpx.Client(timeout=60.0) as client:
                response = client.post(self.API_URL, headers=headers, json=payload)
                response.raise_for_status()

            result = response.json()
            content = result["choices"][0]["message"]["content"]
            data = self._parse_json_response(content)
            return LLMExtractedMetadata(data)

        except httpx.HTTPStatusError as e:
            raise LLMError(f"OpenAI API error: {e.response.status_code}")
        except (KeyError, IndexError) as e:
            raise LLMError(f"Unexpected OpenAI response format: {e}")


# =============================================================================
# Backend Factory
# =============================================================================

BACKENDS = {
    "claude-code": ClaudeCodeParser,
    "openrouter": OpenRouterParser,
    "anthropic": AnthropicParser,
    "openai": OpenAIParser,
}


def get_available_backends() -> list[str]:
    """Get list of available backend names."""
    available = []

    # Check claude-code (local CLI)
    if shutil.which("claude"):
        available.append("claude-code")

    # Check API-based backends
    if os.environ.get("OPENROUTER_API_KEY"):
        available.append("openrouter")
    if os.environ.get("ANTHROPIC_API_KEY"):
        available.append("anthropic")
    if os.environ.get("OPENAI_API_KEY"):
        available.append("openai")

    return available


def get_parser(backend: str | None = None, model: str | None = None) -> LLMParser:
    """Get an LLM parser instance.

    Args:
        backend: Explicit backend name, or None for auto-detection
        model: Model override for API-based backends

    Returns:
        LLMParser instance

    Raises:
        LLMNotAvailableError: If no backend is available
        ValueError: If specified backend is invalid
    """
    if backend:
        if backend not in BACKENDS:
            raise ValueError(f"Unknown backend: {backend}. Available: {list(BACKENDS.keys())}")

        parser_class = BACKENDS[backend]

        # Handle model parameter for API-based backends
        if backend == "claude-code":
            return parser_class()
        else:
            return parser_class(model=model)

    # Auto-detect backend in priority order
    available = get_available_backends()

    if not available:
        raise LLMNotAvailableError(
            "No LLM backend available. Options:\n"
            "  1. Install Claude Code CLI (claude)\n"
            "  2. Set OPENROUTER_API_KEY environment variable\n"
            "  3. Set ANTHROPIC_API_KEY environment variable\n"
            "  4. Set OPENAI_API_KEY environment variable"
        )

    # Use first available backend
    backend = available[0]
    return get_parser(backend=backend, model=model)
