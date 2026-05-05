use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::time::Instant;
use tracing::{debug, info, warn};

use crate::health_check::parse_auth_header;
use ccswitch_db::models::{ApiKey, Provider};
use ccswitch_db::repositories::{ApiKeyRepository, ProviderRepository};

const DEFAULT_PROMPT: &str = "用一句话介绍人工智能的发展历史。";
const DEFAULT_MAX_TOKENS: i64 = 8192;
const DEFAULT_ROUNDS: usize = 3;
const BENCHMARK_TIMEOUT_SECS: u64 = 90;

pub struct BenchmarkService<P, A>
where
    P: ProviderRepository,
    A: ApiKeyRepository,
{
    provider_repo: P,
    api_key_repo: A,
    client: Client,
}

impl<P, A> BenchmarkService<P, A>
where
    P: ProviderRepository,
    A: ApiKeyRepository,
{
    pub fn new(provider_repo: P, api_key_repo: A) -> Self {
        Self {
            provider_repo,
            api_key_repo,
            client: Client::new(),
        }
    }

    pub fn bench_provider(
        &self,
        provider_name: &str,
        prompt: &str,
        max_tokens: i64,
        rounds: usize,
    ) -> Result<ProviderBenchResult> {
        let provider = self
            .provider_repo
            .get_by_name(provider_name)?
            .with_context(|| format!("Provider not found: {}", provider_name))?;

        let key = self
            .api_key_repo
            .get_best_key_for_provider(provider.id)?
            .with_context(|| format!("No active key for provider {}", provider_name))?;

        let model = provider.model.as_deref().unwrap_or("unknown");
        let mut round_results = Vec::new();

        for i in 0..rounds {
            let result = self.run_single_bench(&provider, &key, prompt, max_tokens);
            info!(
                "Bench round {}/{} provider={} ttft={:?}ms total={:?}ms tps={:?}",
                i + 1,
                rounds,
                provider_name,
                result.ttft_ms,
                result.total_ms,
                result.tps,
            );
            round_results.push(result);
        }

        Ok(ProviderBenchResult {
            provider_name: provider.name.clone(),
            display_name: provider.display_name.clone(),
            model: model.to_string(),
            results: round_results,
        })
    }

    pub fn bench_all(
        &self,
        prompt: &str,
        max_tokens: i64,
        rounds: usize,
    ) -> Result<Vec<ProviderBenchResult>> {
        self.bench_all_with_cancel(prompt, max_tokens, rounds, None)
    }

    pub fn bench_all_with_cancel(
        &self,
        prompt: &str,
        max_tokens: i64,
        rounds: usize,
        cancel: Option<&std::sync::atomic::AtomicBool>,
    ) -> Result<Vec<ProviderBenchResult>> {
        let providers = self.provider_repo.list()?;
        let mut all_results = Vec::new();

        for provider in providers {
            if let Some(c) = cancel {
                if c.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
            }
            // Skip native Claude (no base_url) and providers without keys
            if provider.base_url.is_empty() {
                continue;
            }
            if self
                .api_key_repo
                .get_best_key_for_provider(provider.id)
                .ok()
                .flatten()
                .is_none()
            {
                debug!("Skipping {}: no active key", provider.name);
                continue;
            }

            let result = self.bench_provider(&provider.name, prompt, max_tokens, rounds)?;
            all_results.push(result);
        }

        Ok(all_results)
    }

    fn run_single_bench(
        &self,
        provider: &Provider,
        key: &ApiKey,
        prompt: &str,
        max_tokens: i64,
    ) -> BenchResult {
        let url = format!("{}/v1/messages", provider.base_url.trim_end_matches('/'));
        let (header_name, header_value) = parse_auth_header(&provider.auth_header, &key.key_value);
        let model = provider.model.as_deref().unwrap_or("unknown");

        let body = serde_json::json!({
            "model": model,
            "max_tokens": max_tokens,
            "messages": [{ "role": "user", "content": prompt }],
            "stream": true,
        });

        let start = Instant::now();
        let mut ttft_ms: i64 = 0;
        let mut text_parts: Vec<String> = Vec::new();
        let mut output_tokens: i64 = 0;
        let mut first_text_seen = false;

        let response = self
            .client
            .post(&url)
            .header(&header_name, &header_value)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .timeout(std::time::Duration::from_secs(BENCHMARK_TIMEOUT_SECS))
            .send();

        match response {
            Ok(resp) => {
                let status = resp.status();
                if !status.is_success() {
                    let body_text = resp.text().unwrap_or_default();
                    return BenchResult {
                        provider_name: provider.name.clone(),
                        model: model.to_string(),
                        ttft_ms: 0,
                        total_ms: start.elapsed().as_millis() as i64,
                        tokens: 0,
                        tps: 0.0,
                        status: BenchStatus::HttpError(status.as_u16()),
                        error: Some(body_text.chars().take(200).collect()),
                        sample_text: None,
                    };
                }

                // Parse SSE stream
                let reader = BufReader::new(resp);
                let mut sse_error: Option<(String, String)> = None;
                for line_result in reader.lines() {
                    match line_result {
                        Ok(line) => {
                            if let Some(event) = parse_sse_data(&line) {
                                match event {
                                    SseEvent::ContentBlockDelta { delta_type, text } => {
                                        if delta_type == "text_delta" && !text.is_empty() {
                                            if !first_text_seen {
                                                ttft_ms = start.elapsed().as_millis() as i64;
                                                first_text_seen = true;
                                            }
                                            text_parts.push(text);
                                        }
                                    }
                                    SseEvent::MessageDelta { tokens } => {
                                        if tokens > 0 {
                                            output_tokens = tokens;
                                        }
                                    }
                                    SseEvent::MessageStop => break,
                                    SseEvent::Error { code, message } => {
                                        warn!(
                                            "SSE error event for {}: code={} msg={}",
                                            provider.name, code, message
                                        );
                                        sse_error = Some((code, message));
                                        break;
                                    }
                                    SseEvent::Other => {}
                                }
                            }
                        }
                        Err(e) => {
                            warn!("SSE read error for {}: {}", provider.name, e);
                            break;
                        }
                    }
                }

                if let Some((code, message)) = sse_error {
                    let total_ms = start.elapsed().as_millis() as i64;
                    let detail = if code.is_empty() {
                        message
                    } else {
                        format!("{}: {}", code, message)
                    };
                    return BenchResult {
                        provider_name: provider.name.clone(),
                        model: model.to_string(),
                        ttft_ms: 0,
                        total_ms,
                        tokens: 0,
                        tps: 0.0,
                        status: BenchStatus::UpstreamError,
                        error: Some(detail.chars().take(200).collect()),
                        sample_text: None,
                    };
                }

                let total_ms = start.elapsed().as_millis() as i64;
                let sample: String = text_parts.into_iter().collect();
                let tokens = if output_tokens > 0 {
                    output_tokens
                } else {
                    std::cmp::max(1, sample.len() as i64)
                };
                let tps = if total_ms > 0 {
                    tokens as f64 / (total_ms as f64 / 1000.0)
                } else {
                    0.0
                };

                // If the upstream returned HTTP 200 but never produced any text
                // and never emitted a usage block, surface it as an empty stream
                // failure rather than a bogus 1-token success.
                let status = if !first_text_seen && output_tokens == 0 {
                    BenchStatus::EmptyStream
                } else {
                    BenchStatus::Ok
                };

                BenchResult {
                    provider_name: provider.name.clone(),
                    model: model.to_string(),
                    ttft_ms,
                    total_ms,
                    tokens,
                    tps,
                    status,
                    error: if matches!(status, BenchStatus::EmptyStream) {
                        Some("HTTP 200 but no text or token usage in stream".to_string())
                    } else {
                        None
                    },
                    sample_text: if sample.is_empty() {
                        None
                    } else {
                        Some(sample.chars().take(200).collect())
                    },
                }
            }
            Err(e) => {
                let total_ms = start.elapsed().as_millis() as i64;
                BenchResult {
                    provider_name: provider.name.clone(),
                    model: model.to_string(),
                    ttft_ms: 0,
                    total_ms,
                    tokens: 0,
                    tps: 0.0,
                    status: BenchStatus::NetworkError,
                    error: Some(e.to_string()),
                    sample_text: None,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// SSE Parsing
// ---------------------------------------------------------------------------

enum SseEvent {
    ContentBlockDelta { delta_type: String, text: String },
    MessageDelta { tokens: i64 },
    MessageStop,
    Error { code: String, message: String },
    Other,
}

fn parse_sse_data(line: &str) -> Option<SseEvent> {
    let data_str = if line.starts_with("data: ") {
        &line[6..]
    } else if line.starts_with("data:") {
        &line[5..]
    } else {
        return None;
    };

    let data_str = data_str.trim();
    if data_str.is_empty() || data_str == "[DONE]" {
        return None;
    }

    let obj: serde_json::Value = match serde_json::from_str(data_str) {
        Ok(v) => v,
        Err(_) => return None,
    };

    // Some providers (e.g. GLM) emit an SSE `event: error` with a top-level
    // `error` object and no `type` field. Detect that first.
    if let Some(err) = obj.get("error") {
        let code = err
            .get("code")
            .and_then(|v| v.as_str().map(|s| s.to_string()).or_else(|| v.as_i64().map(|i| i.to_string())))
            .unwrap_or_default();
        let message = err
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("upstream error")
            .to_string();
        return Some(SseEvent::Error { code, message });
    }

    let event_type = obj.get("type")?.as_str()?;

    match event_type {
        "content_block_delta" => {
            let delta = obj.get("delta")?;
            let delta_type = delta.get("type")?.as_str()?.to_string();
            let text = match delta_type.as_str() {
                "text_delta" => delta
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                "thinking_delta" => delta
                    .get("thinking")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                _ => String::new(),
            };
            Some(SseEvent::ContentBlockDelta { delta_type, text })
        }
        "message_delta" => {
            let tokens = obj
                .get("usage")
                .and_then(|u| u.get("output_tokens"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            Some(SseEvent::MessageDelta { tokens })
        }
        "message_stop" => Some(SseEvent::MessageStop),
        "error" => {
            let err = obj.get("error").cloned().unwrap_or(serde_json::Value::Null);
            let code = err
                .get("code")
                .and_then(|v| v.as_str().map(|s| s.to_string()).or_else(|| v.as_i64().map(|i| i.to_string())))
                .unwrap_or_default();
            let message = err
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("upstream error")
                .to_string();
            Some(SseEvent::Error { code, message })
        }
        _ => Some(SseEvent::Other),
    }
}

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum BenchStatus {
    Ok,
    HttpError(u16),
    NetworkError,
    UpstreamError,
    EmptyStream,
}

impl std::fmt::Display for BenchStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BenchStatus::Ok => write!(f, "OK"),
            BenchStatus::HttpError(code) => write!(f, "HTTP {}", code),
            BenchStatus::NetworkError => write!(f, "Network Error"),
            BenchStatus::UpstreamError => write!(f, "Upstream Error"),
            BenchStatus::EmptyStream => write!(f, "Empty Stream"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchResult {
    pub provider_name: String,
    pub model: String,
    pub ttft_ms: i64,
    pub total_ms: i64,
    pub tokens: i64,
    pub tps: f64,
    pub status: BenchStatus,
    pub error: Option<String>,
    pub sample_text: Option<String>,
}

impl BenchResult {
    pub fn is_ok(&self) -> bool {
        matches!(self.status, BenchStatus::Ok)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderBenchResult {
    pub provider_name: String,
    pub display_name: String,
    pub model: String,
    pub results: Vec<BenchResult>,
}

impl ProviderBenchResult {
    pub fn avg_ttft_ms(&self) -> f64 {
        let ok: Vec<&BenchResult> = self.results.iter().filter(|r| r.is_ok()).collect();
        if ok.is_empty() {
            return 0.0;
        }
        ok.iter().map(|r| r.ttft_ms as f64).sum::<f64>() / ok.len() as f64
    }

    pub fn avg_total_ms(&self) -> f64 {
        let ok: Vec<&BenchResult> = self.results.iter().filter(|r| r.is_ok()).collect();
        if ok.is_empty() {
            return 0.0;
        }
        ok.iter().map(|r| r.total_ms as f64).sum::<f64>() / ok.len() as f64
    }

    pub fn avg_tps(&self) -> f64 {
        let ok: Vec<&BenchResult> = self.results.iter().filter(|r| r.is_ok()).collect();
        if ok.is_empty() {
            return 0.0;
        }
        ok.iter().map(|r| r.tps).sum::<f64>() / ok.len() as f64
    }

    pub fn avg_tokens(&self) -> f64 {
        let ok: Vec<&BenchResult> = self.results.iter().filter(|r| r.is_ok()).collect();
        if ok.is_empty() {
            return 0.0;
        }
        ok.iter().map(|r| r.tokens as f64).sum::<f64>() / ok.len() as f64
    }

    pub fn success_count(&self) -> usize {
        self.results.iter().filter(|r| r.is_ok()).count()
    }

    pub fn sample_text(&self) -> Option<&str> {
        self.results
            .iter()
            .find(|r| r.is_ok())
            .and_then(|r| r.sample_text.as_deref())
    }
}

pub fn default_prompt() -> &'static str {
    DEFAULT_PROMPT
}

pub fn default_max_tokens() -> i64 {
    DEFAULT_MAX_TOKENS
}

pub fn default_rounds() -> usize {
    DEFAULT_ROUNDS
}
