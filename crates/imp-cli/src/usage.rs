/// Token usage tracking for a session, including prompt caching.
#[derive(Debug, Default, Clone)]
pub struct UsageTracker {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_creation_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub request_count: u32,
    model: Option<String>,
}

/// Per-million-token pricing for a model.
struct Pricing {
    input: f64,
    output: f64,
    cache_write: f64,
    cache_read: f64,
}

fn pricing_for_model(model: &str) -> Pricing {
    // Normalise: strip date suffixes, lowercase
    let m = model.to_lowercase();
    if m.contains("opus-4-5") || m.contains("opus-4.5") {
        Pricing { input: 5.0, output: 25.0, cache_write: 6.25, cache_read: 0.50 }
    } else if m.contains("opus") {
        // Claude 4 Opus
        Pricing { input: 15.0, output: 75.0, cache_write: 18.75, cache_read: 1.50 }
    } else if m.contains("sonnet") {
        Pricing { input: 3.0, output: 15.0, cache_write: 3.75, cache_read: 0.30 }
    } else if m.contains("haiku") {
        Pricing { input: 0.80, output: 4.0, cache_write: 1.0, cache_read: 0.08 }
    } else {
        // Unknown model — use Sonnet as a safe middle ground
        Pricing { input: 3.0, output: 15.0, cache_write: 3.75, cache_read: 0.30 }
    }
}

impl UsageTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_model(&mut self, model: &str) {
        self.model = Some(model.to_string());
    }

    pub fn record(&mut self, input_tokens: u32, output_tokens: u32) {
        self.total_input_tokens += input_tokens as u64;
        self.total_output_tokens += output_tokens as u64;
        self.request_count += 1;
    }

    pub fn record_cache(&mut self, cache_creation: u32, cache_read: u32) {
        self.total_cache_creation_tokens += cache_creation as u64;
        self.total_cache_read_tokens += cache_read as u64;
    }

    pub fn total_tokens(&self) -> u64 {
        self.total_input_tokens
            + self.total_output_tokens
            + self.total_cache_creation_tokens
            + self.total_cache_read_tokens
    }

    fn pricing(&self) -> Pricing {
        pricing_for_model(self.model.as_deref().unwrap_or("opus-4-5"))
    }

    pub fn estimated_cost(&self) -> f64 {
        let p = self.pricing();
        let input_cost = (self.total_input_tokens as f64 / 1_000_000.0) * p.input;
        let output_cost = (self.total_output_tokens as f64 / 1_000_000.0) * p.output;
        let cache_write_cost = (self.total_cache_creation_tokens as f64 / 1_000_000.0) * p.cache_write;
        let cache_read_cost = (self.total_cache_read_tokens as f64 / 1_000_000.0) * p.cache_read;
        input_cost + output_cost + cache_write_cost + cache_read_cost
    }

    /// Format a single response's usage for display.
    pub fn format_response_usage(
        input_tokens: u32,
        output_tokens: u32,
        cache_creation: u32,
        cache_read: u32,
        model: Option<&str>,
    ) -> String {
        let p = pricing_for_model(model.unwrap_or("opus-4-5"));
        let total = input_tokens as u64 + output_tokens as u64
            + cache_creation as u64 + cache_read as u64;

        let cost = (input_tokens as f64 / 1_000_000.0) * p.input
            + (output_tokens as f64 / 1_000_000.0) * p.output
            + (cache_creation as f64 / 1_000_000.0) * p.cache_write
            + (cache_read as f64 / 1_000_000.0) * p.cache_read;

        let mut parts = vec![
            format!("in: {}", input_tokens),
            format!("out: {}", output_tokens),
        ];
        if cache_creation > 0 || cache_read > 0 {
            parts.push(format!("cache: +{}w/{}r", cache_creation, cache_read));
        }

        format!("tokens: {} ({}) · ${:.4}", total, parts.join(", "), cost)
    }

    /// Format session totals for display.
    pub fn format_session_total(&self) -> String {
        let mut parts = vec![
            format!("in: {}", self.total_input_tokens),
            format!("out: {}", self.total_output_tokens),
        ];
        if self.total_cache_creation_tokens > 0 || self.total_cache_read_tokens > 0 {
            parts.push(format!(
                "cache: +{}w/{}r",
                self.total_cache_creation_tokens, self.total_cache_read_tokens
            ));
        }

        format!(
            "Session: {} tokens ({}) · ${:.4} · {} requests",
            self.total_tokens(),
            parts.join(", "),
            self.estimated_cost(),
            self.request_count
        )
    }
}
