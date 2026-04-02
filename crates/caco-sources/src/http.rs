use reqwest::blocking::Client;
use std::time::Duration;

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const DEFAULT_USER_AGENT: &str = "Caco/1.0 (Doom WAD library manager)";

/// Create a shared `reqwest::blocking::Client` with common defaults.
pub fn build_client(timeout_secs: Option<u64>, user_agent: Option<&str>) -> Client {
    Client::builder()
        .timeout(Duration::from_secs(
            timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS),
        ))
        .user_agent(user_agent.unwrap_or(DEFAULT_USER_AGENT))
        .build()
        .expect("failed to build HTTP client")
}

/// Custom serde deserializer: accepts JSON `null` or a `String`, returning
/// an empty `String` for `null`. Mirrors the Python `coerce_str` validator.
pub fn coerce_str<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let opt = Option::<String>::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

/// Custom serde deserializer: accepts JSON `null`, a number, or a numeric
/// string, returning `0` for `null`.
pub fn coerce_i64<'de, D>(deserializer: D) -> std::result::Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let opt = Option::<serde_json::Value>::deserialize(deserializer)?;
    match opt {
        Some(serde_json::Value::Number(n)) => {
            Ok(n.as_i64().unwrap_or(0))
        }
        Some(serde_json::Value::String(s)) => Ok(s.parse::<i64>().unwrap_or(0)),
        _ => Ok(0),
    }
}

/// Custom serde deserializer: accepts JSON `null`, a number, or a numeric
/// string, returning `0.0` for `null`.
pub fn coerce_f64<'de, D>(deserializer: D) -> std::result::Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let opt = Option::<serde_json::Value>::deserialize(deserializer)?;
    match opt {
        Some(serde_json::Value::Number(n)) => {
            Ok(n.as_f64().unwrap_or(0.0))
        }
        Some(serde_json::Value::String(s)) => Ok(s.parse::<f64>().unwrap_or(0.0)),
        _ => Ok(0.0),
    }
}

/// Check if an HTTP response has a Cloudflare WAF challenge.
///
/// Returns `true` when status is 403 and the `cf-mitigated` header is `"challenge"`.
pub fn is_cloudflare_challenged(response: &reqwest::blocking::Response) -> bool {
    response.status() == reqwest::StatusCode::FORBIDDEN
        && response
            .headers()
            .get("cf-mitigated")
            .and_then(|v| v.to_str().ok())
            == Some("challenge")
}

/// Check if an HTTP response has an AWS WAF challenge.
///
/// Returns `true` when status is 403 or the `x-amzn-waf-action` header is `"challenge"`.
pub fn is_aws_waf_challenged(response: &reqwest::blocking::Response) -> bool {
    response.status() == reqwest::StatusCode::FORBIDDEN
        || response
            .headers()
            .get("x-amzn-waf-action")
            .and_then(|v| v.to_str().ok())
            == Some("challenge")
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Deserialize)]
    struct TestStr {
        #[serde(default, deserialize_with = "coerce_str")]
        val: String,
    }

    #[derive(Deserialize)]
    struct TestI64 {
        #[serde(default, deserialize_with = "coerce_i64")]
        val: i64,
    }

    #[derive(Deserialize)]
    struct TestF64 {
        #[serde(default, deserialize_with = "coerce_f64")]
        val: f64,
    }

    #[test]
    fn test_coerce_str_null() {
        let t: TestStr = serde_json::from_str(r#"{"val": null}"#).unwrap();
        assert_eq!(t.val, "");
    }

    #[test]
    fn test_coerce_str_present() {
        let t: TestStr = serde_json::from_str(r#"{"val": "hello"}"#).unwrap();
        assert_eq!(t.val, "hello");
    }

    #[test]
    fn test_coerce_str_missing() {
        let t: TestStr = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(t.val, "");
    }

    #[test]
    fn test_coerce_i64_null() {
        let t: TestI64 = serde_json::from_str(r#"{"val": null}"#).unwrap();
        assert_eq!(t.val, 0);
    }

    #[test]
    fn test_coerce_i64_number() {
        let t: TestI64 = serde_json::from_str(r#"{"val": 42}"#).unwrap();
        assert_eq!(t.val, 42);
    }

    #[test]
    fn test_coerce_f64_null() {
        let t: TestF64 = serde_json::from_str(r#"{"val": null}"#).unwrap();
        assert_eq!(t.val, 0.0);
    }

    #[test]
    fn test_coerce_f64_number() {
        let t: TestF64 = serde_json::from_str(r#"{"val": 4.5}"#).unwrap();
        assert_eq!(t.val, 4.5);
    }
}
