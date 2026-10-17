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
            bail!(
                "claude exited with {} and no result: {}",
                out.status,
                tail(&stderr)
            );
        };

        if result["is_error"] == true || result["subtype"] != "success" {
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
        Ok(Response {
            output,
            cost_usd: result["total_cost_usd"].as_f64(),
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
