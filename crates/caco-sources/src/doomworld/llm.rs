//! LLM-based metadata extraction for Doomworld forum posts.
//!
//! Provides intelligent extraction of WAD metadata from forum posts using
//! various LLM backends. Supplements regex-based extraction with more
//! nuanced understanding of natural language descriptions.
//!
//! Backend priority (auto-detection):
//! 1. Config `[llm]` section
//! 2. `claude` CLI on PATH (claude-code backend)
//! 3. Environment variables: `OPENROUTER_API_KEY`, `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`

use std::process::Command;

use serde_json::Value;

// =============================================================================
// Error Type
// =============================================================================

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    /// No LLM backend is available or configured.
    #[error("LLM not available: {0}")]
    NotAvailable(String),
    /// Extraction/API call failed.
    #[error("LLM extraction failed: {0}")]
    ExtractionFailed(String),
    /// Failed to parse LLM response as JSON.
    #[error("LLM JSON parse error: {0}")]
    JsonParse(String),
}

// =============================================================================
// Extracted Metadata
// =============================================================================

/// Metadata extracted by LLM from a forum post.
#[derive(Debug, Clone, Default)]
pub struct LlmExtractedMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub iwad: Option<String>,
    pub sourceport: Option<String>,
    pub complevel: Option<i32>,
    pub map_count: Option<i32>,
    pub difficulty: Option<String>,
    pub themes: Vec<String>,
    pub download_url: Option<String>,
    pub version: Option<String>,
}

impl LlmExtractedMetadata {
    /// Parse from a JSON value.
    fn from_json(data: &Value) -> Self {
        Self {
            title: json_opt_str(data, "title"),
            author: json_opt_str(data, "author"),
            description: json_opt_str(data, "description"),
            iwad: json_opt_str(data, "iwad"),
            sourceport: json_opt_str(data, "sourceport"),
            complevel: data.get("complevel").and_then(|v| v.as_i64()).map(|v| v as i32),
            map_count: data.get("map_count").and_then(|v| v.as_i64()).map(|v| v as i32),
            difficulty: json_opt_str(data, "difficulty"),
            themes: data
                .get("themes")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
            download_url: json_opt_str(data, "download_url"),
            version: json_opt_str(data, "version"),
        }
    }
}

/// Extract an optional non-null, non-empty string from JSON.
fn json_opt_str(data: &Value, key: &str) -> Option<String> {
    data.get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

// =============================================================================
// Extraction Prompt
// =============================================================================

const EXTRACTION_PROMPT: &str = r#"You are extracting metadata from a Doom WAD release post on the Doomworld forums.

Analyze the following forum post and extract structured metadata. Return a JSON object with these fields:

{
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
}

Important:
- Only include information explicitly stated or strongly implied in the post
- Use null for fields where information is not available
- For iwad/sourceport, use lowercase normalized names
- For themes, use terms like: techbase, hell, gothic, city, abstract, puzzle, slaughter, adventure

Forum post to analyze:
---
{POST_TEXT}
---

Return ONLY the JSON object, no other text."#;

/// Max post text length sent to LLM (~2000 tokens, leaves room for prompt template).
const MAX_POST_TEXT_LEN: usize = 8000;

/// Build the extraction prompt with the given post text (truncated to avoid token limits).
pub(crate) fn build_prompt(post_text: &str) -> String {
    let truncated = if post_text.len() > MAX_POST_TEXT_LEN {
        &post_text[..post_text.floor_char_boundary(MAX_POST_TEXT_LEN)]
    } else {
        post_text
    };
    EXTRACTION_PROMPT.replace("{POST_TEXT}", truncated)
}

// =============================================================================
// LLM Parser Trait
// =============================================================================

/// Trait for LLM parsing backends.
pub trait LlmParser {
    /// Human-readable name of the backend.
    fn name(&self) -> &str;

    /// Parse forum post text and extract metadata.
    fn parse(&self, post_text: &str) -> Result<LlmExtractedMetadata, LlmError>;
}

// =============================================================================
// Shared JSON Response Parser
// =============================================================================

/// Parse JSON from an LLM response, handling markdown code blocks and
/// Claude Code's `{"result": "..."}` wrapper.
fn parse_json_response(response: &str) -> Result<Value, LlmError> {
    let text = response.trim();

    // Handle markdown code blocks: ```json\n{...}\n```
    let unwrapped = if text.starts_with("```") {
        let lines: Vec<&str> = text.lines().collect();
        let start = 1; // skip first line (```json or ```)
        let end = if lines.last().map(|l| l.trim()) == Some("```") {
            lines.len() - 1
        } else {
            lines.len()
        };
        lines[start..end].join("\n")
    } else {
        text.to_string()
    };

    serde_json::from_str::<Value>(&unwrapped)
        .map_err(|e| LlmError::JsonParse(format!("Failed to parse LLM response as JSON: {e}")))
}

// =============================================================================
// Claude Code Backend (Local CLI)
// =============================================================================

/// Uses the local `claude` CLI program.
///
/// Cheapest option for Claude Code subscribers — uses existing subscription
/// rather than API credits.
pub struct ClaudeCodeParser;

impl ClaudeCodeParser {
    /// Check if `claude` CLI is available on PATH.
    pub fn is_available() -> bool {
        caco_core::config::which("claude").is_some()
    }
}

impl LlmParser for ClaudeCodeParser {
    fn name(&self) -> &str {
        "claude-code"
    }

    fn parse(&self, post_text: &str) -> Result<LlmExtractedMetadata, LlmError> {
        let prompt = build_prompt(post_text);

        let output = Command::new("claude")
            .args(["--print", "--output-format", "json", "-p", &prompt])
            .output()
            .map_err(|e| LlmError::ExtractionFailed(format!("Failed to run claude CLI: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(LlmError::ExtractionFailed(format!(
                "Claude CLI failed (exit {}): {stderr}",
                output.status.code().unwrap_or(-1)
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Claude Code outputs JSON with a "result" field containing the text
        let response_text = match serde_json::from_str::<Value>(&stdout) {
            Ok(cli_json) => cli_json
                .get("result")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| stdout.to_string()),
            Err(_) => stdout.to_string(),
        };

        let data = parse_json_response(&response_text)?;
        Ok(LlmExtractedMetadata::from_json(&data))
    }
}

// =============================================================================
// API Backend (OpenRouter / Anthropic / OpenAI)
// =============================================================================

/// Which API provider to use.
#[derive(Debug, Clone, Copy)]
pub enum ApiProvider {
    OpenRouter,
    Anthropic,
    OpenAi,
}

impl ApiProvider {
    fn endpoint(&self) -> &str {
        match self {
            Self::OpenRouter => "https://openrouter.ai/api/v1/chat/completions",
            Self::Anthropic => "https://api.anthropic.com/v1/messages",
            Self::OpenAi => "https://api.openai.com/v1/chat/completions",
        }
    }

    fn default_model(&self) -> &str {
        match self {
            Self::OpenRouter => "anthropic/claude-3-haiku",
            Self::Anthropic => "claude-3-haiku-20240307",
            Self::OpenAi => "gpt-3.5-turbo",
        }
    }

    fn env_var(&self) -> &str {
        match self {
            Self::OpenRouter => "OPENROUTER_API_KEY",
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::OpenAi => "OPENAI_API_KEY",
        }
    }

    fn label(&self) -> &str {
        match self {
            Self::OpenRouter => "openrouter",
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
        }
    }
}

/// API-based LLM parser supporting OpenRouter, Anthropic, and OpenAI.
pub struct ApiParser {
    provider: ApiProvider,
    model: String,
    api_key: String,
    client: reqwest::blocking::Client,
}

impl ApiParser {
    /// Create a new API parser.
    ///
    /// `api_key` can be provided directly or will be read from the
    /// provider's environment variable.
    pub fn new(
        provider: ApiProvider,
        model: Option<&str>,
        api_key: Option<&str>,
    ) -> Result<Self, LlmError> {
        let key = match api_key.filter(|k| !k.is_empty()) {
            Some(k) => k.to_string(),
            None => std::env::var(provider.env_var()).map_err(|_| {
                LlmError::NotAvailable(format!("{} not set", provider.env_var()))
            })?,
        };

        Ok(Self {
            provider,
            model: model
                .filter(|m| !m.is_empty())
                .unwrap_or(provider.default_model())
                .to_string(),
            api_key: key,
            client: crate::http::build_client(Some(60), None),
        })
    }

    /// Build the HTTP request body.
    fn build_request_body(&self, prompt: &str) -> Value {
        match self.provider {
            ApiProvider::Anthropic => serde_json::json!({
                "model": self.model,
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": prompt}],
                "temperature": 0.1,
            }),
            ApiProvider::OpenRouter | ApiProvider::OpenAi => serde_json::json!({
                "model": self.model,
                "messages": [{"role": "user", "content": prompt}],
                "temperature": 0.1,
            }),
        }
    }

    /// Extract the text content from the API response.
    fn extract_content(&self, response: &Value) -> Result<String, LlmError> {
        match self.provider {
            ApiProvider::Anthropic => {
                // Anthropic: content[0].text
                response
                    .get("content")
                    .and_then(|c| c.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|block| block.get("text"))
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string())
                    .ok_or_else(|| {
                        LlmError::ExtractionFailed(
                            "Unexpected Anthropic response format".to_string(),
                        )
                    })
            }
            ApiProvider::OpenRouter | ApiProvider::OpenAi => {
                // OpenAI-style: choices[0].message.content
                response
                    .get("choices")
                    .and_then(|c| c.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|choice| choice.get("message"))
                    .and_then(|msg| msg.get("content"))
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string())
                    .ok_or_else(|| {
                        LlmError::ExtractionFailed(format!(
                            "Unexpected {} response format",
                            self.provider.label()
                        ))
                    })
            }
        }
    }
}

impl LlmParser for ApiParser {
    fn name(&self) -> &str {
        self.provider.label()
    }

    fn parse(&self, post_text: &str) -> Result<LlmExtractedMetadata, LlmError> {
        let prompt = build_prompt(post_text);
        let body = self.build_request_body(&prompt);

        let mut request = self.client.post(self.provider.endpoint());

        // Set auth headers per provider
        match self.provider {
            ApiProvider::Anthropic => {
                request = request
                    .header("x-api-key", &self.api_key)
                    .header("anthropic-version", "2023-06-01");
            }
            ApiProvider::OpenRouter => {
                request = request
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .header("HTTP-Referer", "https://github.com/eshen/caco")
                    .header("X-Title", "Caco WAD Library Manager");
            }
            ApiProvider::OpenAi => {
                request = request.header("Authorization", format!("Bearer {}", self.api_key));
            }
        }

        let response = request
            .json(&body)
            .send()
            .map_err(|e| LlmError::ExtractionFailed(format!("{} API error: {e}", self.provider.label())))?;

        if !response.status().is_success() {
            return Err(LlmError::ExtractionFailed(format!(
                "{} API returned status {}",
                self.provider.label(),
                response.status()
            )));
        }

        let response_json: Value = response
            .json()
            .map_err(|e| LlmError::ExtractionFailed(format!("Failed to parse API response: {e}")))?;

        let content = self.extract_content(&response_json)?;
        let data = parse_json_response(&content)?;
        Ok(LlmExtractedMetadata::from_json(&data))
    }
}

// =============================================================================
// Factory
// =============================================================================

/// Get an LLM parser instance.
///
/// The caller is responsible for resolving config values and passing them
/// as parameters. This function handles:
/// 1. Explicit `backend`/`api_key`/`model` parameters
/// 2. Auto-detection: `claude` CLI on PATH
/// 3. Auto-detection: environment variables
pub fn get_parser(
    backend: Option<&str>,
    model: Option<&str>,
    api_key: Option<&str>,
) -> Result<Box<dyn LlmParser>, LlmError> {
    // If explicit backend given, use it directly
    if let Some(be) = backend.filter(|b| !b.is_empty()) {
        return create_parser(be, model, api_key);
    }

    // Auto-detect: claude CLI
    if ClaudeCodeParser::is_available() {
        return Ok(Box::new(ClaudeCodeParser));
    }

    // Auto-detect: env vars
    for provider in [ApiProvider::OpenRouter, ApiProvider::Anthropic, ApiProvider::OpenAi] {
        if std::env::var(provider.env_var()).is_ok() {
            return Ok(Box::new(ApiParser::new(provider, model, api_key)?));
        }
    }

    Err(LlmError::NotAvailable(
        "No LLM backend available. Options:\n  \
         1. Install Claude Code CLI (claude)\n  \
         2. Set OPENROUTER_API_KEY environment variable\n  \
         3. Set ANTHROPIC_API_KEY environment variable\n  \
         4. Set OPENAI_API_KEY environment variable\n  \
         5. Configure [llm] section in config.toml"
            .to_string(),
    ))
}

/// Create a parser for a specific backend name.
fn create_parser(
    backend: &str,
    model: Option<&str>,
    api_key: Option<&str>,
) -> Result<Box<dyn LlmParser>, LlmError> {
    match backend {
        "claude-code" => Ok(Box::new(ClaudeCodeParser)),
        "openrouter" => Ok(Box::new(ApiParser::new(ApiProvider::OpenRouter, model, api_key)?)),
        "anthropic" => Ok(Box::new(ApiParser::new(ApiProvider::Anthropic, model, api_key)?)),
        "openai" => Ok(Box::new(ApiParser::new(ApiProvider::OpenAi, model, api_key)?)),
        other => Err(LlmError::NotAvailable(format!(
            "Unknown backend: {other}. Available: claude-code, openrouter, anthropic, openai"
        ))),
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extraction_prompt_format() {
        let prompt = build_prompt("A cool Doom map for GZDoom");
        assert!(prompt.contains("A cool Doom map for GZDoom"));
        assert!(prompt.contains("Return ONLY the JSON object"));
        assert!(prompt.contains("\"iwad\""));
        assert!(prompt.contains("\"complevel\""));
    }

    #[test]
    fn test_extraction_prompt_truncation() {
        let long_text = "a".repeat(10000);
        let prompt = build_prompt(&long_text);
        // The prompt should contain at most 8000 chars of post text
        // (plus the template text)
        assert!(prompt.len() < 10000 + EXTRACTION_PROMPT.len());
        // Should not contain the full 10000 chars
        assert!(!prompt.contains(&"a".repeat(9000)));
    }

    #[test]
    fn test_parse_json_response_raw() {
        let json = r#"{"title": "Cool WAD", "author": "Mapper", "complevel": 9}"#;
        let result = parse_json_response(json).unwrap();
        assert_eq!(result["title"], "Cool WAD");
        assert_eq!(result["author"], "Mapper");
        assert_eq!(result["complevel"], 9);
    }

    #[test]
    fn test_parse_json_response_markdown_wrapped() {
        let json = "```json\n{\"title\": \"Test\", \"iwad\": \"doom2\"}\n```";
        let result = parse_json_response(json).unwrap();
        assert_eq!(result["title"], "Test");
        assert_eq!(result["iwad"], "doom2");
    }

    #[test]
    fn test_parse_json_response_markdown_no_lang() {
        let json = "```\n{\"title\": \"NoLang\"}\n```";
        let result = parse_json_response(json).unwrap();
        assert_eq!(result["title"], "NoLang");
    }

    #[test]
    fn test_parse_json_response_invalid() {
        let result = parse_json_response("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn test_llm_metadata_from_json() {
        let data = serde_json::json!({
            "title": "Sunlust",
            "author": "Ribbiks",
            "description": "32 challenging maps",
            "iwad": "doom2",
            "sourceport": "dsda-doom",
            "complevel": 9,
            "map_count": 32,
            "difficulty": "slaughter",
            "themes": ["techbase", "hell"],
            "download_url": "https://example.com/sunlust.zip",
            "version": "v1.0"
        });

        let meta = LlmExtractedMetadata::from_json(&data);
        assert_eq!(meta.title.as_deref(), Some("Sunlust"));
        assert_eq!(meta.author.as_deref(), Some("Ribbiks"));
        assert_eq!(meta.iwad.as_deref(), Some("doom2"));
        assert_eq!(meta.complevel, Some(9));
        assert_eq!(meta.map_count, Some(32));
        assert_eq!(meta.themes, vec!["techbase", "hell"]);
        assert_eq!(meta.version.as_deref(), Some("v1.0"));
    }

    #[test]
    fn test_llm_metadata_defaults() {
        let data = serde_json::json!({});
        let meta = LlmExtractedMetadata::from_json(&data);
        assert!(meta.title.is_none());
        assert!(meta.author.is_none());
        assert!(meta.complevel.is_none());
        assert!(meta.themes.is_empty());
    }

    #[test]
    fn test_llm_metadata_null_fields() {
        let data = serde_json::json!({
            "title": null,
            "iwad": null,
            "complevel": null,
            "themes": null,
        });
        let meta = LlmExtractedMetadata::from_json(&data);
        assert!(meta.title.is_none());
        assert!(meta.iwad.is_none());
        assert!(meta.complevel.is_none());
        assert!(meta.themes.is_empty());
    }

    #[test]
    fn test_get_parser_no_backend() {
        // In test environment, typically no backends are available.
        // Set env to ensure nothing is found.
        let result = get_parser(Some("nonexistent"), None, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_parser_unknown_backend() {
        let result = create_parser("foobar", None, None);
        assert!(matches!(result, Err(LlmError::NotAvailable(_))));
    }

    #[test]
    fn test_api_provider_defaults() {
        assert_eq!(ApiProvider::OpenRouter.default_model(), "anthropic/claude-3-haiku");
        assert_eq!(ApiProvider::Anthropic.default_model(), "claude-3-haiku-20240307");
        assert_eq!(ApiProvider::OpenAi.default_model(), "gpt-3.5-turbo");
    }

    #[test]
    fn test_api_provider_env_vars() {
        assert_eq!(ApiProvider::OpenRouter.env_var(), "OPENROUTER_API_KEY");
        assert_eq!(ApiProvider::Anthropic.env_var(), "ANTHROPIC_API_KEY");
        assert_eq!(ApiProvider::OpenAi.env_var(), "OPENAI_API_KEY");
    }

    #[test]
    fn test_api_parser_extract_content_anthropic() {
        let parser = ApiParser {
            provider: ApiProvider::Anthropic,
            model: "test".to_string(),
            api_key: "test".to_string(),
            client: crate::http::build_client(Some(60), None),
        };

        let response = serde_json::json!({
            "content": [{"type": "text", "text": "{\"title\": \"Test\"}"}]
        });
        let content = parser.extract_content(&response).unwrap();
        assert_eq!(content, "{\"title\": \"Test\"}");
    }

    #[test]
    fn test_api_parser_extract_content_openai() {
        let parser = ApiParser {
            provider: ApiProvider::OpenAi,
            model: "test".to_string(),
            api_key: "test".to_string(),
            client: crate::http::build_client(Some(60), None),
        };

        let response = serde_json::json!({
            "choices": [{"message": {"content": "{\"title\": \"Test\"}"}}]
        });
        let content = parser.extract_content(&response).unwrap();
        assert_eq!(content, "{\"title\": \"Test\"}");
    }

    #[test]
    fn test_api_parser_extract_content_bad_format() {
        let parser = ApiParser {
            provider: ApiProvider::Anthropic,
            model: "test".to_string(),
            api_key: "test".to_string(),
            client: crate::http::build_client(Some(60), None),
        };

        let response = serde_json::json!({"unexpected": "format"});
        let result = parser.extract_content(&response);
        assert!(result.is_err());
    }
}
