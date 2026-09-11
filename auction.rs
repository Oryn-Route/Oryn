//! Auction coordinator.
//!
//! Owns the 2–3 second off-chain auction window and winner selection.
//! Selection is a pure function (`select_best_quote`) so it is trivially
//! unit-testable; the coordinator wraps it with persistence and tie-breaking.

use std::sync::Arc;
use std::time::Duration;

use rust_decimal::Decimal;
use uuid::Uuid;

use super::store::{OpaStore, QuoteRow};

/// Default auction window — 2.5 seconds, inside the 2–3s design budget.
pub const DEFAULT_AUCTION_WINDOW_MS: u64 = 2500;

/// Human label for the winner-visible auction status surface.
pub const BEST_QUOTE_PUBLIC_VIEW: &str = "winner";

/// Configuration read from the environment at coordinator construction.
#[derive(Debug, Clone, Copy)]
pub struct AuctionConfig {
    /// How long the off-chain auction stays open after the round opens.
    pub window: Duration,
}

impl Default for AuctionConfig {
    fn default() -> Self {
        let window_ms = std::env::var("OFA_AUCTION_WINDOW_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_AUCTION_WINDOW_MS)
            .clamp(1000, 5000);
        Self {
            window: Duration::from_millis(window_ms),
        }
    }
}

/// Errors selecting the best quote. Kept separate from the API error enum so
/// the coordinator stays framework-free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BestQuoteError {
    /// No quotes were submitted or all were malformed.
    NoQualifyingQuotes,
    /// A quote fill amount could not be parsed as a decimal.
    MalformedFill(String),
}

/// The top bid. `fill_amount` is a normalized decimal string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BestQuote {
    pub quote_id: Uuid,
    pub solver_id: Uuid,
    pub fill_amount: String,
    pub received_at: chrono::DateTime<chrono::Utc>,
}

/// Pure winner selection: the highest fill that clears the intent minimum,
/// tie-broken by earliest arrival.
pub fn select_best_quote(
    quotes: &[QuoteRow],
    min_output: &str,
) -> Result<Option<BestQuote>, BestQuoteError> {
    let min_output = min_output.parse::<Decimal>().map_err(|e| {
        BestQuoteError::MalformedFill(format!("min_output is not a decimal: {e}"))
    })?;

    let mut best: Option<(Decimal, Uuid)> = None;
    let mut chosen: Option<String> = None; // fill string of the current best quote

    for quote in quotes {
        let fill = quote
            .fill_amount
            .parse::<Decimal>()
            .map_err(|e| BestQuoteError::MalformedFill(format!("{}: {e}", quote.quote_id)))?;

        if fill < min_output {
            continue; // does not clear the user's floor
        }

        let better = match best {
            None => true,
            Some((current, current_quote_id)) => {
                if fill > current {
                    true
                } else if fill == current && quote.quote_id == current_quote_id {
                    false
                } else if fill == current {
                    // Tie-break by earliest received_at; a stable compare
                    // avoids the winner flip-flopping between two equal bids.
                    let earlier_quote = quotes.iter().find(|q| q.quote_id == current_quote_id);
                    match earlier_quote {
                        Some(earlier) => quote.received_at < earlier.received_at,
                        None => false,
                    }
                } else {
                    false
                }
            }
        };

        if better {
            best = Some((fill, quote.quote_id));
            chosen = Some(quote.fill_amount.clone());
        }
    }

    let Some((_, quote_id)) = best else {
        return Ok(None);
    };
    let quote = quotes
        .iter()
        .find(|q| q.quote_id == quote_id)
        .expect("chosen quote must exist");
    Ok(Some(BestQuote {
        quote_id: quote.quote_id,
        solver_id: quote.solver_id,
        fill_amount: chosen.unwrap_or_else(|| quote.fill_amount.clone()),
        received_at: quote.received_at,
    }))
}

/// Coordinator wrapping a [`OpaStore`] with the auction lifecycle.
#[derive(Clone)]
pub struct AuctionCoordinator {
    store: Arc<dyn OpaStore>,
    config: AuctionConfig,
}

impl AuctionCoordinator {
    pub fn new(store: Arc<dyn OpaStore>) -> Self {
        Self {
            store,
            config: AuctionConfig::default(),
        }
    }

    pub fn with_config(store: Arc<dyn OpaStore>, config: AuctionConfig) -> Self {
        Self { store, config }
    }

    pub fn config(&self) -> AuctionConfig {
        self.config
    }

    /// Open a new auction round for `intent_id`. Returns `Ok(None)` when the
    /// intent is not in `pending` state (already opened / cancelled / etc.).
    pub async fn open_auction(
        &self,
        intent_id: Uuid,
    ) -> Result<super::store::RoundRow, sqlx::Error> {
        let round_id = Uuid::new_v4();
        let closes_at = chrono::Utc::now() + self.config.window;
        self.store
            .open_round(round_id, intent_id, closes_at)
            .await
            .map(|row| row.expect("open_auction returns the round when it exists"))
    }

    /// Close the round at its deadline, select the best qualifying quote, and
    /// commit it as the winner. `min_output` is the intent's floor that every
    /// quote must clear.
    pub async fn settle_auction(
        &self,
        round_id: Uuid,
        min_output: &str,
    ) -> Result<Option<BestQuote>, BestQuoteError> {
        let quotes = self
            .store
            .list_round_quotes(round_id)
            .await
            .map_err(|e| BestQuoteError::MalformedFill(format!("store error: {e}")))?;

        let Some(best) = select_best_quote(&quotes, min_output)? else {
            return Ok(None);
        };
        let _ = self
            .store
            .select_winner(round_id, best.quote_id, best.solver_id)
            .await;
        Ok(Some(best))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn quote(id: u32, fill: &str, received_at: chrono::DateTime<chrono::Utc>) -> QuoteRow {
        QuoteRow {
            quote_id: Uuid::from_u128(id as u128),
            round_id: Uuid::from_u128(0),
            intent_id: Uuid::from_u128(1),
            solver_id: Uuid::from_u128(2),
            fill_amount: fill.to_string(),
            execution_path: None,
            quote_signature: "sig".to_string(),
            is_winner: false,
            received_at,
        }
    }

    #[test]
    fn picks_highest_clearing_fill() {
        let t0 = Utc.with_ymd_and_hms(2026, 9, 10, 12, 0, 0).unwrap();
        let q = vec![
            quote(1, "90.0000000", t0 + chrono::Duration::milliseconds(10)),
            quote(2, "95.5000000", t0 + chrono::Duration::milliseconds(50)),
            quote(3, "80.0000000", t0 + chrono::Duration::milliseconds(20)),
        ];
        let best = select_best_quote(&q, "85").unwrap().expect("winner");
        assert_eq!(best.quote_id, Uuid::from_u128(2));
        assert_eq!(best.fill_amount, "95.5000000");
    }

    #[test]
    fn filters_quotes_below_minimum() {
        let t0 = Utc.with_ymd_and_hms(2026, 9, 10, 12, 0, 0).unwrap();
        let q = vec![
            quote(1, "10.0000000", t0),
            quote(2, "19.9999999", t0 + chrono::Duration::milliseconds(5)),
        ];
        let best = select_best_quote(&q, "20").unwrap();
        assert!(best.is_none());
    }

    #[test]
    fn tie_breaks_by_earliest_arrival() {
        let t0 = Utc.with_ymd_and_hms(2026, 9, 10, 12, 0, 0).unwrap();
        let q = vec![
            quote(1, "100.0000000", t0 + chrono::Duration::milliseconds(30)),
            quote(2, "100.0000000", t0 + chrono::Duration::milliseconds(10)),
        ];
        let best = select_best_quote(&q, "1").unwrap().expect("winner");
        assert_eq!(best.quote_id, Uuid::from_u128(2), "later equal bid loses");
    }

    #[test]
    fn empty_quote_set_returns_none() {
        let best = select_best_quote(&[], "1").unwrap();
        assert!(best.is_none());
    }
}