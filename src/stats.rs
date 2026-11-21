//! Token counters, shared by every aggregation level.

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Stats {
    #[serde(default)]
    pub input_tokens: i64,
    #[serde(default)]
    pub output_tokens: i64,
    #[serde(default)]
    pub cache_read_tokens: i64,
    #[serde(default)]
    pub cache_creation_tokens: i64,
    #[serde(default)]
    pub context_input_tokens: i64,
    #[serde(default)]
    pub requests: i64,
}

impl Stats {
    /// Tokens a human thinks of as "usage" for the menu bar number.
    pub fn in_plus_out(&self) -> i64 {
        self.input_tokens + self.output_tokens
    }

    /// Fold one model request's metrics in.
    pub fn add(
        &mut self,
        input: i64,
        output: i64,
        cache_read: i64,
        cache_create: i64,
        context: i64,
    ) {
        self.input_tokens += input;
        self.output_tokens += output;
        self.cache_read_tokens += cache_read;
        self.cache_creation_tokens += cache_create;
        self.context_input_tokens += context;
        self.requests += 1;
    }

    pub fn merge(&mut self, other: &Stats) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_creation_tokens += other.cache_creation_tokens;
        self.context_input_tokens += other.context_input_tokens;
        self.requests += other.requests;
    }
}

/// "1234567" -> "1.2M"
pub fn compact(n: i64) -> String {
    let v = n as f64;
    if v >= 1e9 {
        format!("{:.1}B", v / 1e9)
    } else if v >= 1e6 {
        format!("{:.1}M", v / 1e6)
    } else if v >= 1e3 {
        format!("{:.1}K", v / 1e3)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_counts_a_request() {
        let mut s = Stats::default();
        s.add(10, 5, 100, 0, 110);
        assert_eq!(s.input_tokens, 10);
        assert_eq!(s.in_plus_out(), 15);
        assert_eq!(s.requests, 1);
    }

    #[test]
    fn merge_sums_everything() {
        let mut a = Stats {
            input_tokens: 1,
            requests: 2,
            ..Default::default()
        };
        let b = Stats {
            input_tokens: 3,
            output_tokens: 4,
            requests: 5,
            ..Default::default()
        };
        a.merge(&b);
        assert_eq!(a.input_tokens, 4);
        assert_eq!(a.requests, 7);
    }

    #[test]
    fn compact_formats() {
        assert_eq!(compact(999), "999");
        assert_eq!(compact(1_234), "1.2K");
        assert_eq!(compact(4_605_395), "4.6M");
        assert_eq!(compact(15_566_752_059), "15.6B");
    }
}
