/// Token usage tracking for a session
#[derive(Debug, Default, Clone)]
pub struct UsageTracker {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub request_count: u32,
}

impl UsageTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, input_tokens: u32, output_tokens: u32) {
        self.total_input_tokens += input_tokens as u64;
        self.total_output_tokens += output_tokens as u64;
        self.request_count += 1;
    }

    pub fn total_tokens(&self) -> u64 {
        self.total_input_tokens + self.total_output_tokens
    }

    /// Estimate cost based on Claude Opus 4.5 pricing
    /// Input: $15/MTok, Output: $75/MTok
    pub fn estimated_cost(&self) -> f64 {
        let input_cost = (self.total_input_tokens as f64 / 1_000_000.0) * 15.0;
        let output_cost = (self.total_output_tokens as f64 / 1_000_000.0) * 75.0;
        input_cost + output_cost
    }

    /// Format a single response's usage for display
    pub fn format_response_usage(input_tokens: u32, output_tokens: u32) -> String {
        let total = input_tokens + output_tokens;
        let cost = (input_tokens as f64 / 1_000_000.0) * 15.0
            + (output_tokens as f64 / 1_000_000.0) * 75.0;
        format!(
            "tokens: {} (in: {}, out: {}) · ${:.4}",
            total, input_tokens, output_tokens, cost
        )
    }

    /// Format session totals for display
    pub fn format_session_total(&self) -> String {
        format!(
            "Session total: {} tokens (in: {}, out: {}) · ${:.4} · {} requests",
            self.total_tokens(),
            self.total_input_tokens,
            self.total_output_tokens,
            self.estimated_cost(),
            self.request_count
        )
    }
}
