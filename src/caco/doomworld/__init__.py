"""Doomworld forum client and parser."""

from caco.doomworld.client import DoomworldClient, DoomworldError
from caco.doomworld.models import ForumThread
from caco.doomworld.parser import (
    DoomworldParser,
    extract_download_links,
    extract_complevel,
    extract_iwad,
    extract_sourceport,
    complevel_name,
    iwad_display_name,
    sourceport_display_name,
)
from caco.doomworld.llm import (
    LLMParser,
    LLMError,
    LLMNotAvailableError,
    LLMExtractedMetadata,
    ClaudeCodeParser,
    OpenRouterParser,
    AnthropicParser,
    OpenAIParser,
    get_parser,
    get_available_backends,
)

__all__ = [
    # Client
    "DoomworldClient",
    "DoomworldError",
    # Parser
    "DoomworldParser",
    "ForumThread",
    # Extraction functions
    "extract_download_links",
    "extract_complevel",
    "extract_iwad",
    "extract_sourceport",
    # Display name helpers
    "complevel_name",
    "iwad_display_name",
    "sourceport_display_name",
    # LLM parsing (Phase 3)
    "LLMParser",
    "LLMError",
    "LLMNotAvailableError",
    "LLMExtractedMetadata",
    "ClaudeCodeParser",
    "OpenRouterParser",
    "AnthropicParser",
    "OpenAIParser",
    "get_parser",
    "get_available_backends",
]
