//! Runs headless Claude Code, signed in with the owner's Claude subscription
//! (`CLAUDE_CODE_OAUTH_TOKEN` from `claude setup-token`), with an optional image,
//! and returns JSON matching a schema. There is no Rust SDK and subscription
//! tokens only work through Claude Code, so this shells out to the `claude` CLI.

use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, bail};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub struct Image<'a> {
    pub media_type: &'a str,
    pub base64: &'a str,
}

pub struct Request<'a> {
    pub system: &'a str,
    pub prompt: &'a str,
    pub image: Option<Image<'a>>,
    pub schema: &'a Value,
    /// low, medium, high, xhigh or max.
    pub effort: &'a str,
}

pub struct Response {
    pub output: Value,
    /// What the run would have cost on the API; billed to the subscription instead.
    pub cost_usd: Option<f64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}

/// The subscription's usage limit was reached. Returned inside the `anyhow::Error`;
/// check with `err.downcast_ref::<UsageLimit>()` and retry after `resets_at`.
#[derive(Debug)]
pub struct UsageLimit {
    pub message: String,
    /// Unix seconds, when Claude Code reported it.
    pub resets_at: Option<i64>,
}

impl std::fmt::Display for UsageLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Claude usage limit reached: {}", self.message)
    }
}

impl std::error::Error for UsageLimit {}

/// Claude Code reports a hit limit as an error result such as
/// "Claude AI usage limit reached|1760720400" or "You've hit your limit · resets 6pm".
fn usage_limit(text: &str) -> Option<UsageLimit> {
    let lower = text.to_lowercase();
    if !(lower.contains("usage limit")
        || lower.contains("hit your limit")
        || lower.contains("rate limit"))
    {
        return None;
    }
    let resets_at = text
        .rsplit('|')
        .next()
        .and_then(|t| t.trim().parse::<i64>().ok())
        .filter(|t| *t > 1_000_000_000);
    Some(UsageLimit {
        message: text.trim().chars().take(300).collect(),
        resets_at,
    })
}

pub struct Claude {
    bin: String,
    model: Option<String>,
    timeout: Duration,
}

impl Claude {
    pub fn from_env() -> Self {
        Self {
            bin: std::env::var("CLAUDE_BIN").unwrap_or_else(|_| "claude".into()),
            model: std::env::var("CLAUDE_MODEL").ok().filter(|m| !m.is_empty()),
            timeout: Duration::from_secs(15 * 60),
        }
    }

    pub async fn version(&self) -> anyhow::Result<String> {
        let out = Command::new(&self.bin).arg("--version").output().await?;
        if !out.status.success() {
            bail!("claude --version exited with {}", out.status);
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    pub async fn run(&self, req: Request<'_>) -> anyhow::Result<Response> {
        let schema = serde_json::to_string(req.schema)?;
        let mut cmd = Command::new(&self.bin);
        cmd.args([
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--tools",
            "",
            "--no-session-persistence",
            "--strict-mcp-config",
            "--effort",
            req.effort,
            "--system-prompt",
            req.system,
            "--json-schema",
            &schema,
        ]);
        if let Some(model) = &self.model {
            cmd.args(["--model", model]);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut content = Vec::new();
        if let Some(image) = &req.image {
            content.push(json!({
                "type": "image",
                "source": { "type": "base64", "media_type": image.media_type, "data": image.base64 },
            }));
        }
        content.push(json!({ "type": "text", "text": req.prompt }));
        let message = json!({ "type": "user", "message": { "role": "user", "content": content } });

        let mut child = cmd.spawn().context("starting claude")?;
        let mut stdin = child.stdin.take().expect("stdin is piped");
        stdin.write_all(format!("{message}\n").as_bytes()).await?;
        drop(stdin);

        let out = tokio::time::timeout(self.timeout, child.wait_with_output())
            .await
            .context("claude timed out")??;

        let stdout = String::from_utf8_lossy(&out.stdout);
        let Some(result) = stdout
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .find(|e| e["type"] == "result")
        else {
            let stderr = String::from_utf8_lossy(&out.stderr);
            if let Some(limit) = usage_limit(&stderr) {
                return Err(limit.into());
            }
            bail!(
                "claude exited with {} and no result: {}",
                out.status,
                tail(&stderr)
            );
        };

        if result["is_error"] == true || result["subtype"] != "success" {
            if let Some(limit) = usage_limit(result["result"].as_str().unwrap_or_default()) {
                return Err(limit.into());
            }
            bail!(
                "claude failed ({}): {}",
                result["subtype"],
                tail(&result["result"].to_string())
            );
        }
        let output = match &result["structured_output"] {
            Value::Null => serde_json::from_str(result["result"].as_str().unwrap_or_default())
                .context("claude returned no structured output")?,
            v => v.clone(),
        };
        let usage = &result["usage"];
        let input_tokens = [
            "input_tokens",
            "cache_creation_input_tokens",
            "cache_read_input_tokens",
        ]
        .iter()
        .filter_map(|k| usage[k].as_i64())
        .reduce(|a, b| a + b);
        Ok(Response {
            output,
            cost_usd: result["total_cost_usd"].as_f64(),
            input_tokens,
            output_tokens: usage["output_tokens"].as_i64(),
        })
    }
}

fn tail(s: &str) -> &str {
    let start = s.len().saturating_sub(500);
    let start = (start..s.len())
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(s.len());
    &s[start..]
}

#[cfg(test)]
mod tests {
    use super::usage_limit;

    #[test]
    fn detects_usage_limits() {
        let l = usage_limit("Claude AI usage limit reached|1760720400").unwrap();
        assert_eq!(l.resets_at, Some(1760720400));
        let l = usage_limit("You've hit your limit · resets 6pm (Africa/Lagos)").unwrap();
        assert_eq!(l.resets_at, None);
        assert!(usage_limit("Invalid JSON schema").is_none());
    }
}
