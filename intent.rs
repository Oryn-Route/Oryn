//! Order Flow Auction (OFA) models.
//!
//! An *intent* is a signed-but-unbroadcast swap expression. Solvers compete
//! during a 2–3 second off-chain auction and the winning quote is committed on
//! chain. Wire amounts/timestamps are scalars (decimal strings, unix epoch
//! milliseconds) so the OpenAPI surface stays dependency-free and matches the
//! rest of the v2 seam — this crate treats the exact CAIP-2 / CAIP-19 strings
//! as opaque validated identifiers.

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use utoipa::ToSchema;

/// Lifecycle of an intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum IntentStatus {
    /// Submitted by the user, no auction opened yet.
    Pending,
    /// An auction round is open and accepting solver quotes.
    AuctionOpen,
    /// The auction closed and a winning solver was selected.
    AuctionWon,
    /// The winning solver delivered the fill on-chain.
    Filled,
    /// Auction closed without a qualifying quote.
    Abandoned,
    /// The deadline passed before settlement.
    Expired,
    /// Cancelled by the user or an operator.
    Cancelled,
}

impl IntentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            IntentStatus::Pending => "pending",
            IntentStatus::AuctionOpen => "auction_open",
            IntentStatus::AuctionWon => "auction_won",
            IntentStatus::Filled => "filled",
            IntentStatus::Abandoned => "abandoned",
            IntentStatus::Expired => "expired",
            IntentStatus::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "pending" => IntentStatus::Pending,
            "auction_open" => IntentStatus::AuctionOpen,
            "auction_won" => IntentStatus::AuctionWon,
            "filled" => IntentStatus::Filled,
            "abandoned" => IntentStatus::Abandoned,
            "expired" => IntentStatus::Expired,
            "cancelled" => IntentStatus::Cancelled,
            _ => return None,
        })
    }
}

/// Solver account state mirrored from the on-chain registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SolverStatus {
    Active,
    Suspended,
    Slashed,
}

impl SolverStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SolverStatus::Active => "active",
            SolverStatus::Suspended => "suspended",
            SolverStatus::Slashed => "slashed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "active" => SolverStatus::Active,
            "suspended" => SolverStatus::Suspended,
            "slashed" => SolverStatus::Slashed,
            _ => return None,
        })
    }
}

/// Round lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RoundStatus {
    Open,
    Closed,
    Settled,
    #[serde(rename = "expired")]
    ExpiredNoWinner,
}

impl RoundStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RoundStatus::Open => "open",
            RoundStatus::Closed => "closed",
            RoundStatus::Settled => "settled",
            RoundStatus::ExpiredNoWinner => "expired",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "open" => RoundStatus::Open,
            "closed" => RoundStatus::Closed,
            "settled" => RoundStatus::Settled,
            "expired" => RoundStatus::ExpiredNoWinner,
            _ => return None,
        })
    }
}

/// POST /api/v2/intents body.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateIntentRequest {
    /// User controlling the source funds on `from_chain`.
    pub user_address: String,
    /// Canonical CAIP-19 source asset (e.g. `stellar:pubnet/...:...`).
    pub from_asset: String,
    /// CAIP-2 source chain. One of `stellar`, `eip155`, `bip122`, `solana`, `tron`.
    pub from_chain: String,
    /// Amount of `from_asset` to spend (decimal string, scale preserved).
    pub from_amount: String,
    /// Canonical CAIP-19 destination asset.
    pub to_asset: String,
    /// CAIP-2 destination chain.
    pub to_chain: String,
    /// Destination address receiving the fill.
    pub to_address: String,
    /// Auction/settlement deadline as unix epoch milliseconds.
    pub deadline_ms: i64,
    /// Minimum acceptable output of `to_asset` (decimal string).
    pub min_output: String,
    /// Ed25519 signature over the intent digest, if provided.
    #[serde(default)]
    pub signature: Option<String>,
    /// Client-supplied nonce; unique per (from_chain, user_address).
    pub nonce: i64,
}

/// POST /api/v2/intents response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateIntentResponse {
    pub intent_id: Uuid,
    pub status: IntentStatus,
    pub created_at_ms: i64,
}

/// GET /api/v2/intents/:id response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IntentResponse {
    pub intent_id: Uuid,
    pub user_address: String,
    pub from_asset: String,
    pub from_asset_canonical: String,
    pub from_chain: String,
    pub from_amount: String,
    pub to_asset: String,
    pub to_asset_canonical: String,
    pub to_chain: String,
    pub to_address: String,
    pub deadline_ms: i64,
    pub min_output: String,
    pub signature: Option<String>,
    pub nonce: i64,
    pub status: IntentStatus,
    pub cancel_reason: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// POST /api/v2/auctions/:intent_id/open response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OpenAuctionResponse {
    pub round_id: Uuid,
    pub intent_id: Uuid,
    pub opened_at_ms: i64,
    /// Auction closes at this unix epoch millisecond (now + configured window).
    pub closes_at_ms: i64,
    pub status: RoundStatus,
}

/// POST /api/v2/solvers/register body.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RegisterSolverRequest {
    pub name: String,
    /// Registry home chain id (`stellar:pubnet` or `solana:mainnet`).
    pub home_chain: String,
    /// On-chain address that registered the bond (registry contract account).
    pub bond_address: String,
    /// Bond asset identifier; `native` (XLM/SOL) by default.
    #[serde(default = "default_bond_asset")]
    pub bond_asset: String,
    /// Staked bond amount as reported by the registry.
    pub bond_amount: String,
    /// API key used to authenticate quote submissions.
    pub api_key: String,
}

fn default_bond_asset() -> String {
    "native".to_string()
}

/// Solver record as stored/searched.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SolverResponse {
    pub solver_id: Uuid,
    pub name: String,
    pub home_chain: String,
    pub bond_address: String,
    pub bond_asset: String,
    pub bond_amount: String,
    pub status: SolverStatus,
    pub registered_at_ms: i64,
    pub last_heartbeat_at_ms: Option<i64>,
}

/// POST /api/v2/auctions/:round_id/quotes body.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SubmitQuoteRequest {
    /// Fill amount of the destination asset (decimal string).
    pub fill_amount: String,
    /// Free-form execution path description (optional, informational).
    #[serde(default)]
    pub execution_path: Option<String>,
    /// Solver signature over (intent_id, round_id, fill_amount).
    pub quote_signature: String,
}

/// A single solver bid.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SolverQuoteResponse {
    pub quote_id: Uuid,
    pub round_id: Uuid,
    pub solver_id: Uuid,
    pub solver_name: String,
    pub fill_amount: String,
    pub is_winner: bool,
    pub received_at_ms: i64,
}

/// GET /api/v2/auctions/:round_id/quotes response — winner-visible view.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AuctionStatusResponse {
    pub round_id: Uuid,
    pub intent_id: Uuid,
    pub status: RoundStatus,
    pub opens_at_ms: i64,
    pub closes_at_ms: i64,
    pub winner: Option<SolverQuoteResponse>,
    pub quote_count: usize,
    pub view: &'static str,
}

/// POST /api/v2/auctions/:round_id/settle body.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SettleAuctionRequest {
    /// On-chain settlement transaction hash.
    pub tx_hash: String,
    /// Chain where settlement landed (CAIP-2).
    pub chain: String,
}

/// POST /api/v2/auctions/:round_id/settle response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SettleAuctionResponse {
    pub round_id: Uuid,
    pub intent_id: Uuid,
    pub status: RoundStatus,
    pub settled_at_ms: i64,
    pub tx_hash: Option<String>,
}