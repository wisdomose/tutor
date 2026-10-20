//! Minimal OpenAI Responses API client: one request in, JSON matching a schema out,
//! with token usage and its price.

use std::time::Duration;

use anyhow::{Context, bail};
use serde::Deserialize;
use serde_json::{Value, json};

pub const SOL: &str = "gpt-6.1-sol";
pub const LUNA: &str = "gpt-6-luna";

/// Standard short-context prices in USD per 1M tokens (input, output).
fn price(model: &str) -> Option<(f64, f64)> {
    match model {
        SOL => Some((2.00, 10.00)),
        LUNA => Some((0.10, 0.50)),
        _ => None,
    }
}

pub enum Part<'a> {
    Text(&'a str),
    /// Base64 JPEG.
    Jpeg(&'a str),
}

pub struct Request<'a> {
    pub model: &'a str,
    pub instructions: &'a str,
    pub input: Vec<Part<'a>>,
    /// Name and JSON schema of the structured output.
    pub schema_name: &'a str,
    pub schema: &'a Value,
}

pub struct Response {
    pub output: Value,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub cost_usd: Option<f64>,
}

#[derive(Clone)]
pub struct OpenAi {
    http: reqwest::Client,
    key: String,
    /// `OPENAI_BASE_URL`, for a local stand-in when testing; the real API otherwise.
    base_url: String,
}

impl OpenAi {
    pub fn from_env() -> anyhow::Result<Self> {
        let key = std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY must be set")?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10 * 60))
            .build()?;
        let base_url = std::env::var("OPENAI_BASE_URL")
            .ok()
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| "https://api.openai.com/v1".into());
        Ok(Self {
            http,
            key,
            base_url,
        })
    }

    pub async fn respond(&self, req: Request<'_>) -> anyhow::Result<Response> {
        let content: Vec<Value> = req
            .input
            .iter()
            .map(|part| match part {
                Part::Text(t) => json!({ "type": "input_text", "text": t }),
                Part::Jpeg(b64) => json!({
                    "type": "input_image",
                    "image_url": format!("data:image/jpeg;base64,{b64}"),
                    "detail": "high",
                }),
            })
            .collect();
        let body = json!({
            "model": req.model,
            "instructions": req.instructions,
            "input": [{ "role": "user", "content": content }],
            "text": { "format": {
                "type": "json_schema", "name": req.schema_name, "schema": req.schema, "strict": true,
            }},
        });

        let mut attempt = 0;
        let raw: RawResponse = loop {
            attempt += 1;
            let res = self
                .http
                .post(format!("{}/responses", self.base_url))
                .bearer_auth(&self.key)
                .json(&body)
                .send()
                .await;
            let retryable = match res {
                Ok(r) if r.status().is_success() => break r.json().await?,
                Ok(r) => {
                    let status = r.status();
                    let text = r.text().await.unwrap_or_default();
                    if status.as_u16() != 429 && !status.is_server_error() {
                        bail!("openai {} {status}: {}", req.model, truncate(&text));
                    }
                    format!("{status}: {}", truncate(&text))
                }
                Err(e) => e.to_string(),
            };
            if attempt >= 4 {
                bail!(
                    "openai {} failed after {attempt} attempts: {retryable}",
                    req.model
                );
            }
            tracing::warn!(model = req.model, attempt, "openai retry: {retryable}");
            tokio::time::sleep(Duration::from_secs(5 * attempt)).await;
        };

        if raw.status.as_deref() == Some("incomplete") {
            bail!("openai {} returned an incomplete response", req.model);
        }
        let text = raw
            .output
            .iter()
            .filter(|o| o.kind == "message")
            .flat_map(|o| o.content.iter())
            .find_map(|c| match c.kind.as_str() {
                "output_text" => c.text.clone(),
                "refusal" => Some(String::new()),
                _ => None,
            })
            .context("openai response had no output text")?;
        if text.is_empty() {
            bail!("openai {} refused the request", req.model);
        }
        let output = serde_json::from_str(&text).context("openai output was not valid JSON")?;
        let (input_tokens, output_tokens) = (raw.usage.input_tokens, raw.usage.output_tokens);
        let cost_usd = price(req.model)
            .map(|(i, o)| input_tokens as f64 / 1e6 * i + output_tokens as f64 / 1e6 * o);
        Ok(Response {
            output,
            input_tokens,
            output_tokens,
            cost_usd,
        })
    }
}

#[derive(Deserialize)]
struct RawResponse {
    status: Option<String>,
    output: Vec<RawOutput>,
    usage: RawUsage,
}

#[derive(Deserialize)]
struct RawOutput {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    content: Vec<RawContent>,
}

#[derive(Deserialize)]
struct RawContent {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct RawUsage {
    input_tokens: i32,
    output_tokens: i32,
}

fn truncate(s: &str) -> &str {
    let end = (0..=s.len().min(400))
        .rev()
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(0);
    &s[..end]
}
