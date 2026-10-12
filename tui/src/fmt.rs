//! Number and text formatting helpers

use chrono::NaiveDate;

const THOUSAND: f64 = 1_000.0;
const MILLION: f64 = 1_000_000.0;

/// Insert thousands separators into a string of digits
fn group_digits(digits: &str) -> String {
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// `15,472`
pub fn count(value: usize) -> String {
    group_digits(&value.to_string())
}

/// `$1,234.56`, with a leading minus for negatives
pub fn money(value: f64) -> String {
    let cents = (value.abs() * 100.0).round() as u64;
    let sign = if value < 0.0 && cents > 0 { "-" } else { "" };
    format!("{}${}.{:02}", sign, group_digits(&(cents / 100).to_string()), cents % 100)
}

/// `$1,235` (whole dollars)
pub fn money0(value: f64) -> String {
    let dollars = value.abs().round() as u64;
    let sign = if value < 0.0 && dollars > 0 { "-" } else { "" };
    format!("{}${}", sign, group_digits(&dollars.to_string()))
}

/// `+$1,234.56` / `-$1,234.56`
pub fn signed_money(value: f64) -> String {
    if value > 0.0 {
        format!("+{}", money(value))
    } else {
        money(value)
    }
}

/// `$1.2M`, `$45.3K`, `$950`
pub fn compact(value: f64) -> String {
    let magnitude = value.abs();
    let sign = if value < 0.0 { "-" } else { "" };
    if magnitude >= MILLION {
        format!("{}${:.2}M", sign, magnitude / MILLION)
    } else if magnitude >= THOUSAND {
        format!("{}${:.1}K", sign, magnitude / THOUSAND)
    } else {
        format!("{}${:.0}", sign, magnitude)
    }
}

/// `+1.2%` / `-0.4%`
pub fn signed_pct(value: f64) -> String {
    format!("{}{:.2}%", if value > 0.0 { "+" } else { "" }, value)
}

/// Shorten to at most `max` characters, marking the cut with an ellipsis
pub fn trunc(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else if max == 0 {
        String::new()
    } else {
        let mut out: String = text.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}

/// Horizontal bar `width` cells wide filled to `fraction`, in eighth-cell steps
pub fn hbar(fraction: f64, width: usize) -> String {
    const EIGHTHS: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];
    let eighths = (fraction.clamp(0.0, 1.0) * width as f64 * 8.0).round() as usize;
    let mut bar = "█".repeat(eighths / 8);
    if eighths % 8 > 0 {
        bar.push(EIGHTHS[eighths % 8]);
    }
    let pad = width.saturating_sub(bar.chars().count());
    bar.push_str(&" ".repeat(pad));
    bar
}

/// `Oct 11`
pub fn short_date(date: NaiveDate) -> String {
    date.format("%b %-d").to_string()
}

/// `Oct 11, 2026`
pub fn long_date(date: NaiveDate) -> String {
    date.format("%b %-d, %Y").to_string()
}

/// `Monthly`, `Twice a month`, ...
pub fn frequency(raw: &str) -> String {
    match raw {
        "semimonthly_start_mid" | "semimonthly_mid_end" => "Twice a month".to_string(),
        "biweekly" => "Every 2 weeks".to_string(),
        other => {
            let mut chars = other.replace('_', " ").chars().collect::<Vec<_>>();
            if let Some(first) = chars.first_mut() {
                *first = first.to_ascii_uppercase();
            }
            chars.into_iter().collect()
        }
    }
}
