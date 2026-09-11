use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HorizonPriceR {
    pub n: i64,
    pub d: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HorizonOffer {
    pub id: String,
    pub paging_token: Option<String>,
    pub seller: String,

    pub selling: serde_json::Value,
    pub buying: serde_json::Value,

    pub amount: String,
    pub price: String,

    pub price_r: Option<HorizonPriceR>,
    pub last_modified_ledger: i64,
    pub last_modified_time: Option<String>,
    pub sponsor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HorizonEmbedded<T> {
    pub records: Vec<T>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HorizonLinks {
    #[serde(rename = "next")]
    pub next: Option<HorizonLink>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HorizonLink {
    pub href: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HorizonPage<T> {
    #[serde(rename = "_embedded")]
    pub embedded: HorizonEmbedded<T>,
    #[serde(rename = "_links")]
    pub links: Option<HorizonLinks>,
}

/// A single bid or ask level in an orderbook
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrderbookLevel {
    pub price_r: HorizonPriceR,
    pub price: String,
    pub amount: String,
}

/// Typed asset descriptor returned by Horizon
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HorizonAsset {
    pub asset_type: String,
    pub asset_code: Option<String>,
    pub asset_issuer: Option<String>,
}

/// Typed orderbook snapshot returned by `GET /order_book`
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HorizonOrderbook {
    pub bids: Vec<OrderbookLevel>,
    pub asks: Vec<OrderbookLevel>,
    pub base: HorizonAsset,
    pub counter: HorizonAsset,
}

impl HorizonOrderbook {
    /// Returns `true` when both bid and ask sides are empty
    pub fn is_empty(&self) -> bool {
        self.bids.is_empty() && self.asks.is_empty()
    }

    /// Best bid price (highest bid), if any
    pub fn best_bid(&self) -> Option<&str> {
        self.bids.first().map(|l| l.price.as_str())
    }

    /// Best ask price (lowest ask), if any
    pub fn best_ask(&self) -> Option<&str> {
        self.asks.first().map(|l| l.price.as_str())
    }

    /// Mid price calculated as (best_bid + best_ask) / 2, returns `None` when
    /// either side is empty or the prices cannot be parsed.
    pub fn mid_price(&self) -> Option<f64> {
        let bid: f64 = self.best_bid()?.parse().ok()?;
        let ask: f64 = self.best_ask()?.parse().ok()?;
        Some((bid + ask) / 2.0)
    }
}
