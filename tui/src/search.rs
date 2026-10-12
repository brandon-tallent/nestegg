//! Matching a typed search against the rows of a screen
//!
//! A search is a handful of words, each of which must appear somewhere in the
//! row, in any order and ignoring case and punctuation. A word written as an
//! amount (`>100`, `<50`, `$45`, `$45.99`) is tested against the row's amount
//! instead.

/// What a word of the search asks of a row's amount
#[derive(Debug, Clone, Copy, PartialEq)]
enum AmountTest {
    Above(f64),
    Below(f64),
    /// At least the first and under the second
    Within(f64, f64),
}

/// Half a cent: amounts closer than this are the same amount
const CENT_TOLERANCE: f64 = 0.005;

/// One word of a search, as typed and with its punctuation closed up
#[derive(Debug, Clone, PartialEq)]
struct Word {
    /// "in-shape" as "in shape", "2026-08" as "2026 08"
    spaced: String,
    /// "in-shape" as "inshape"; only for words with a letter in them, since
    /// digits run together across a date or a card number by accident
    squashed: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Query {
    words: Vec<Word>,
    amounts: Vec<AmountTest>,
}

impl Query {
    pub fn parse(text: &str) -> Self {
        let mut query = Query::default();
        for word in text.split_whitespace() {
            match amount_test(word) {
                Some(test) => query.amounts.push(test),
                None => {
                    let spaced = normalize(word).trim().to_string();
                    if !spaced.is_empty() {
                        let squashed = spaced.chars().any(char::is_alphabetic).then(|| spaced.replace(' ', ""));
                        query.words.push(Word { spaced, squashed });
                    }
                }
            }
        }
        query
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty() && self.amounts.is_empty()
    }

    /// Whether a row with these texts and this amount satisfies the search.
    /// The sign of the amount is ignored: `>100` finds a $150 charge.
    pub fn matches(&self, fields: &[&str], amount: Option<f64>) -> bool {
        if self.is_empty() {
            return true;
        }
        let size = amount.map(f64::abs);
        let amounts_pass = self.amounts.iter().all(|test| match (test, size) {
            // A row with no amount cannot fail a test of it
            (_, None) => true,
            (AmountTest::Above(limit), Some(size)) => size > *limit,
            (AmountTest::Below(limit), Some(size)) => size < *limit,
            (AmountTest::Within(low, high), Some(size)) => size >= *low && size < *high,
        });
        if !amounts_pass || self.words.is_empty() {
            return amounts_pass;
        }
        // Spaced for whole words, squashed so "inshape" finds "In-Shape"
        let spaced: Vec<String> = fields.iter().map(|field| normalize(field)).collect();
        let squashed: Vec<String> = spaced.iter().map(|field| field.replace(' ', "")).collect();
        self.words.iter().all(|word| {
            spaced.iter().any(|field| field.contains(&word.spaced))
                || word.squashed.as_ref().is_some_and(|w| squashed.iter().any(|field| field.contains(w)))
        })
    }
}

/// Lowercase with apostrophes dropped and other punctuation turned to spaces,
/// so "Trader Joe's" reads "trader joes" and "In-Shape" reads "in shape"
fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if c != '\'' && c != '’' && !out.ends_with(' ') {
            out.push(' ');
        }
    }
    out
}

fn amount_test(word: &str) -> Option<AmountTest> {
    let (comparison, rest) = match word.chars().next()? {
        c @ ('>' | '<' | '=' | '$') => (c, &word[c.len_utf8()..]),
        _ => return None,
    };
    let digits = rest.trim_start_matches('$').replace(',', "");
    let value: f64 = digits.parse().ok()?;
    Some(match comparison {
        '>' => AmountTest::Above(value),
        '<' => AmountTest::Below(value),
        // "$45" means forty-five and change; "$45.99" means exactly that
        _ if digits.contains('.') => AmountTest::Within(value - CENT_TOLERANCE, value + CENT_TOLERANCE),
        _ => AmountTest::Within(value, value + 1.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(search: &str, fields: &[&str], amount: Option<f64>) -> bool {
        Query::parse(search).matches(fields, amount)
    }

    #[test]
    fn words_match_in_any_order_across_fields() {
        let row = ["Rollover 401k", "Fidelity", "Investments"];
        assert!(found("fidelity 401", &row, None));
        assert!(found("INVEST rollover", &row, None));
        assert!(!found("fidelity roth", &row, None));
    }

    #[test]
    fn punctuation_and_spacing_do_not_matter() {
        assert!(found("trader joes", &["Trader Joe's"], None));
        assert!(found("inshape", &["In-Shape Fitness"], None));
        assert!(found("in-shape", &["In-Shape Fitness"], None));
        assert!(found("walmart+", &["Walmart+"], None));
    }

    #[test]
    fn digits_do_not_run_together_across_a_field() {
        let date = ["2024-01-15 Jan January"];
        assert!(found("2024-01", &date, None));
        assert!(found("jan 2024", &date, None));
        assert!(!found("401", &date, None));
        assert!(found("401", &["Rollover 401k"], None));
    }

    #[test]
    fn amount_words_test_the_amount_whatever_its_sign() {
        assert!(found(">100", &["Rent"], Some(-150.0)));
        assert!(!found(">100", &["Rent"], Some(-50.0)));
        assert!(found("<50 coffee", &["Coffee Shop"], Some(-4.5)));
        assert!(found("$45", &["Shop"], Some(-45.99)));
        assert!(!found("$45", &["Shop"], Some(-46.0)));
        assert!(found("$45.99", &["Shop"], Some(45.99)));
        assert!(!found("$45.99", &["Shop"], Some(45.0)));
        assert!(found(">1,000", &["Shop"], Some(2_000.0)));
    }

    #[test]
    fn empty_search_matches_everything() {
        assert!(Query::parse("  ").is_empty());
        assert!(found("", &["Anything"], None));
        // Punctuation alone asks for nothing
        assert!(found("-", &["Anything"], None));
    }
}
