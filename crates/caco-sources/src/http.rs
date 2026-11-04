use reqwest::blocking::{Client, Response};
use std::fmt;
use std::time::Duration;

pub(crate) const TIMEOUT: Duration = Duration::from_secs(30);

/// Who caco says it is to every site it talks to. One spelling, so the version
/// and the project URL cannot go stale in one client and not in another.
pub const USER_AGENT: &str = concat!(
    "Caco/",
    env!("CARGO_PKG_VERSION"),
    " (Doom WAD library manager; https://github.com/mythofmeat/caco)"
);

/// Create a shared `reqwest::blocking::Client` with common defaults.
pub fn build_client() -> Client {
    Client::builder()
        .timeout(TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .expect("failed to build HTTP client")
}

/// Custom serde deserializer: accepts JSON `null`, a `String`, or a number, returning an
/// empty `String` for `null`.
///
/// A number because the idgames API writes anything that looks like one as one: a WAD
/// called `2014` has `"title": 2014`, and an author called `8130423` the same. Mirrors the
/// Python `coerce_str` validator otherwise.
pub fn coerce_str<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let opt = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match opt {
        Some(serde_json::Value::String(s)) => s,
        Some(serde_json::Value::Number(n)) => n.to_string(),
        Some(serde_json::Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    })
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
        Some(serde_json::Value::Number(n)) => Ok(n.as_i64().unwrap_or(0)),
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
        Some(serde_json::Value::Number(n)) => Ok(n.as_f64().unwrap_or(0.0)),
        Some(serde_json::Value::String(s)) => Ok(s.parse::<f64>().unwrap_or(0.0)),
        _ => Ok(0.0),
    }
}

/// The WAF behind a [`waf_challenge`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WafKind {
    Cloudflare,
    AwsWaf,
}

impl fmt::Display for WafKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Cloudflare => "Cloudflare",
            Self::AwsWaf => "AWS WAF",
        })
    }
}

/// Recognise a response that is a WAF's challenge rather than the site's answer.
///
/// Knows every WAF, not just the one a site is behind today: a site can change
/// provider without notice, and a client checking for the old one then reports
/// the challenge as a bare HTTP error.
///
/// The headers are the whole signal, and the status is deliberately not part of
/// it. Cloudflare documents `cf-mitigated` without naming a status. AWS WAF
/// answers a challenge with 202 and a CAPTCHA with 405. A 202 passes
/// `error_for_status`, so callers must ask this first or the interstitial
/// reaches their parser.
pub fn waf_challenge(response: &Response) -> Option<WafKind> {
    let header = |name| response.headers().get(name)?.to_str().ok();

    if header("cf-mitigated") == Some("challenge") {
        Some(WafKind::Cloudflare)
    } else if matches!(header("x-amzn-waf-action"), Some("challenge" | "captcha")) {
        Some(WafKind::AwsWaf)
    } else {
        None
    }
}

/// Build a real [`Response`] without a network, for testing what a client does
/// with one.
#[cfg(test)]
pub(crate) fn canned_response(
    status: u16,
    headers: &[(&str, &str)],
    body: &'static str,
) -> Response {
    let mut response = ::http::Response::builder().status(status);
    for &(name, value) in headers {
        response = response.header(name, value);
    }
    response.body(body).expect("valid canned response").into()
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    /// A real challenge, as caco's own client received it on 2026-10-31.
    #[test]
    fn waf_challenge_recognises_cloudflare() {
        let response = canned_response(
            403,
            &[("server", "cloudflare"), ("cf-mitigated", "challenge")],
            "<html>Just a moment...</html>",
        );
        assert_eq!(waf_challenge(&response), Some(WafKind::Cloudflare));
    }

    /// Cloudflare documents the header and no status, so a different status must
    /// not hide a challenge.
    #[test]
    fn waf_challenge_recognises_cloudflare_at_any_status() {
        let response = canned_response(503, &[("cf-mitigated", "challenge")], "");
        assert_eq!(waf_challenge(&response), Some(WafKind::Cloudflare));
    }

    /// AWS documents 202 for a challenge. The old 403 gate could never match it.
    #[test]
    fn waf_challenge_recognises_an_aws_challenge() {
        let response = canned_response(202, &[("x-amzn-waf-action", "challenge")], "");
        assert_eq!(waf_challenge(&response), Some(WafKind::AwsWaf));
    }

    /// AWS documents 405 for a CAPTCHA, which caco can solve no better.
    #[test]
    fn waf_challenge_recognises_an_aws_captcha() {
        let response = canned_response(405, &[("x-amzn-waf-action", "captcha")], "");
        assert_eq!(waf_challenge(&response), Some(WafKind::AwsWaf));
    }

    /// Being behind a WAF is not being challenged by it: a site that answers
    /// normally can still send `server: cloudflare`.
    #[test]
    fn waf_challenge_ignores_a_normal_answer_from_behind_a_waf() {
        let response = canned_response(200, &[("server", "cloudflare")], "{}");
        assert_eq!(waf_challenge(&response), None);
    }

    /// A real refusal keeps its status. Blaming a challenge that never happened
    /// would send the user looking for the wrong cause.
    #[test]
    fn waf_challenge_ignores_a_plain_403() {
        let response = canned_response(403, &[("server", "cloudflare")], "Forbidden");
        assert_eq!(waf_challenge(&response), None);
    }

    #[test]
    fn waf_challenge_ignores_values_it_does_not_know() {
        for header in [("cf-mitigated", "block"), ("x-amzn-waf-action", "allow")] {
            let response = canned_response(403, &[header], "");
            assert_eq!(waf_challenge(&response), None, "{header:?}");
        }
    }

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

    /// `vtm_2014.zip` on the real API, and `hexndm.zip`'s author is `8130423`.
    #[test]
    fn test_coerce_str_takes_what_the_api_wrote_as_a_number() {
        let t: TestStr = serde_json::from_str(r#"{"val": 2014}"#).unwrap();
        assert_eq!(t.val, "2014");
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
