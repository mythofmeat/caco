"""Doomworld forum client and parser."""

from caco.doomworld.client import DoomworldClient, DoomworldError
from caco.doomworld.models import ForumThread
from caco.complevel import complevel_name  # noqa: F401
from caco.doomworld.parser import (
    COMPLEVEL_NAMES,
    DoomworldParser,
    extract_download_links,
    extract_complevel,
    extract_iwad,
    extract_sourceport,
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
    # Constants
    "COMPLEVEL_NAMES",
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
