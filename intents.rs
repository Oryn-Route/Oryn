//! Order Flow Auction (OFA) HTTP surface.
//!
//! Endpoints:
//! - `POST /api/v2/intents` — submit a signed-but-unbroadcast intent
//! - `GET /api/v2/intents/{intent_id}` — intent status
//! - `POST /api/v2/auctions/{intent_id}/open` — open the 2–3s auction window
//! - `POST /api/v2/solvers/register` — register a solver (bond mirrored on-chain)
//! - `GET /api/v2/solvers/{solver_id}` — solver record
//! - `POST /api/v2/auctions/{round_id}/quotes` — solver bid (authed)
//! - `GET /api/v2/auctions/{round_id}/quotes` — winner-visible auction status
//! - `POST /api/v2/auctions/{round_id}/settle` — winner/operator settlement report

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::error::{ApiError, Result};
use crate::middleware::RequestId;
use crate::models::{
    ApiResponse, AuctionStatusResponse, CreateIntentRequest, CreateIntentResponse, IntentResponse,
    OpenAuctionResponse, RegisterSolverRequest, RoundStatus, SettleAuctionRequest,
    SettleAuctionResponse, SolverQuoteResponse, SolverResponse, SubmitQuoteRequest,
};
use crate::ofa::auction::BEST_QUOTE_PUBLIC_VIEW;
use crate::ofa::store::{hash_api_key, NewIntent, NewQuote, NewSolver};
use crate::ofa::{SOLVER_ID_HEADER, SOLVER_KEY_HEADER};
use crate::state::AppState;

use orynroute_routing::chain_asset::ChainAsset;

use std::sync::Arc;

fn to_ms(dt: DateTime<Utc>) -> i64 {
    dt.timestamp_millis()
}

/// Validate the intent body. Returns the two canonical asset ids.
fn validate_intent_request(body: &CreateIntentRequest) -> Result<(String, String)> {
    let now = Utc::now();
    let from_chain = orynroute_routing::chain_asset::ChainId::parse_caip2(&body.from_chain)
        .map_err(|e| ApiError::InvalidIntent(format!("invalid from_chain: {e}")))?;
    let _to_chain = orynroute_routing::chain_asset::ChainId::parse_caip2(&body.to_chain)
        .map_err(|e| ApiError::InvalidIntent(format!("invalid to_chain: {e}")))?;

    let from = ChainAsset::parse(&body.from_asset)
        .map_err(|e| ApiError::InvalidAsset(format!("from_asset: {e}")))?;
    if from.chain.to_caip2() != from_chain.to_caip2() {
        return Err(ApiError::InvalidIntent(
            "from_asset chain must match from_chain".to_string(),
        ));
    }
    let to = ChainAsset::parse(&body.to_asset)
        .map_err(|e| ApiError::InvalidAsset(format!("to_asset: {e}")))?;
    let to_canonical = to.to_canonical();
    let from_canonical = from.to_canonical();

    let from_amount = body
        .from_amount
        .parse::<Decimal>()
        .map_err(|_| ApiError::InvalidAmount("from_amount must be a decimal".to_string()))?;
    if from_amount <= Decimal::ZERO {
        return Err(ApiError::InvalidAmount(
            "from_amount must be positive".to_string(),
        ));
    }

    let min_output = body
        .min_output
        .parse::<Decimal>()
        .map_err(|_| ApiError::InvalidAmount("min_output must be a decimal".to_string()))?;
    if min_output < Decimal::ZERO {
        return Err(ApiError::InvalidAmount(
            "min_output must not be negative".to_string(),
        ));
    }

    let deadline = DateTime::from_timestamp_millis(body.deadline_ms)
        .ok_or_else(|| ApiError::Validation("deadline_ms is not a valid unix millis".to_string()))?;
    if deadline <= now {
        return Err(ApiError::IntentExpired {
            intent_id: "new".to_string(),
            deadline_ms: body.deadline_ms,
        });
    }
    if body.to_address.trim().is_empty() {
        return Err(ApiError::Validation("to_address must not be empty".to_string()));
    }
    if body.nonce < 0 {
        return Err(ApiError::Validation("nonce must not be negative".to_string()));
    }

    Ok((from_canonical, to_canonical))
}

/// `POST /api/v2/intents`
#[utoipa::path(
    post,
    path = "/api/v2/intents",
    tag = "ofa",
    request_body = CreateIntentRequest,
    responses(
        (status = 200, body = ApiResponse<CreateIntentResponse>),
        (status = 400, description = "invalid intent"),
        (status = 422, description = "intent expired"),
    ),
)]
pub async fn create_intent(
    State(state): State<Arc<AppState>>,
    request_id: RequestId,
    Json(body): Json<CreateIntentRequest>,
) -> Result<Json<ApiResponse<CreateIntentResponse>>> {
    let (from_canonical, to_canonical) = validate_intent_request(&body)?;

    let intent_id = Uuid::new_v4();
    let deadline = DateTime::from_timestamp_millis(body.deadline_ms)
        .ok_or_else(|| ApiError::Validation("deadline_ms invalid".to_string()))?;

    let created = state
        .ofa_store
        .create_intent(&NewIntent {
            intent_id,
            user_address: body.user_address.clone(),
            from_asset: body.from_asset.clone(),
            from_asset_canonical: from_canonical,
            from_chain: body.from_chain.clone(),
            from_amount: body.from_amount.clone(),
            to_asset: body.to_asset.clone(),
            to_asset_canonical: to_canonical,
            to_chain: body.to_chain.clone(),
            to_address: body.to_address.clone(),
            deadline,
            min_output: body.min_output.clone(),
            signature: body.signature.clone(),
            nonce: body.nonce,
        })
        .await?;

    Ok(Json(ApiResponse::with_version(
        2,
        CreateIntentResponse {
            intent_id,
            status: created.status(),
            created_at_ms: to_ms(created.created_at),
        },
        request_id.as_str(),
    )))
}

/// `GET /api/v2/intents/{intent_id}`
#[utoipa::path(
    get,
    path = "/api/v2/intents/{intent_id}",
    tag = "ofa",
    params(("intent_id" = Uuid, Path, description = "Intent UUID")),
    responses(
        (status = 200, body = ApiResponse<IntentResponse>),
        (status = 404, description = "intent_not_found"),
    ),
)]
pub async fn get_intent(
    State(state): State<Arc<AppState>>,
    request_id: RequestId,
    Path(intent_id): Path<Uuid>,
) -> Result<Json<ApiResponse<IntentResponse>>> {
    let intent = state
        .ofa_store
        .get_intent(intent_id)
        .await?
        .ok_or_else(|| ApiError::IntentNotFound {
            intent_id: intent_id.to_string(),
        })?;

    let status = intent.status();
    Ok(Json(ApiResponse::with_version(
        2,
        IntentResponse {
            intent_id: intent.intent_id,
            user_address: intent.user_address,
            from_asset: intent.from_asset,
            from_asset_canonical: intent.from_asset_canonical,
            from_chain: intent.from_chain,
            from_amount: intent.from_amount,
            to_asset: intent.to_asset,
            to_asset_canonical: intent.to_asset_canonical,
            to_chain: intent.to_chain,
            to_address: intent.to_address,
            deadline_ms: to_ms(intent.deadline),
            min_output: intent.min_output,
            signature: intent.signature,
            nonce: intent.nonce,
            status,
            cancel_reason: intent.cancel_reason,
            created_at_ms: to_ms(intent.created_at),
            updated_at_ms: to_ms(intent.updated_at),
        },
        request_id.as_str(),
    )))
}

/// `POST /api/v2/auctions/{intent_id}/open`
#[utoipa::path(
    post,
    path = "/api/v2/auctions/{intent_id}/open",
    tag = "ofa",
    params(("intent_id" = Uuid, Path, description = "Intent UUID")),
    responses(
        (status = 200, body = ApiResponse<OpenAuctionResponse>),
        (status = 404, description = "intent_not_found"),
        (status = 409, description = "intent already auctioned"),
    ),
)]
pub async fn open_auction(
    State(state): State<Arc<AppState>>,
    request_id: RequestId,
    Path(intent_id): Path<Uuid>,
) -> Result<Json<ApiResponse<OpenAuctionResponse>>> {
    let intent = state
        .ofa_store
        .get_intent(intent_id)
        .await?
        .ok_or_else(|| ApiError::IntentNotFound {
            intent_id: intent_id.to_string(),
        })?;

    if intent.status() != crate::models::IntentStatus::Pending {
        return Err(ApiError::Conflict {
            message: "intent is not pending; it cannot be auctioned again".to_string(),
            quote_id: intent_id.to_string(),
            tx_hash: String::new(),
            status: intent.status,
        });
    }
    if intent.deadline <= Utc::now() {
        return Err(ApiError::IntentExpired {
            intent_id: intent_id.to_string(),
            deadline_ms: to_ms(intent.deadline),
        });
    }

    let round = state.auction_coordinator.open_auction(intent_id).await?;

    Ok(Json(ApiResponse::with_version(
        2,
        OpenAuctionResponse {
            round_id: round.round_id,
            intent_id: round.intent_id,
            opened_at_ms: to_ms(round.opened_at),
            closes_at_ms: to_ms(round.closes_at),
            status: RoundStatus::Open,
        },
        request_id.as_str(),
    )))
}

/// `POST /api/v2/solvers/register`
#[utoipa::path(
    post,
    path = "/api/v2/solvers/register",
    tag = "ofa",
    request_body = RegisterSolverRequest,
    responses(
        (status = 200, body = ApiResponse<SolverResponse>),
        (status = 400, description = "invalid registration"),
    ),
)]
pub async fn register_solver(
    State(state): State<Arc<AppState>>,
    request_id: RequestId,
    Json(body): Json<RegisterSolverRequest>,
) -> Result<Json<ApiResponse<SolverResponse>>> {
    // The home chain must be a registry-bearing chain (Stellar or Solana for
    // this release). Other chains are persisted but ignored by auction routing.
    orynroute_routing::chain_asset::ChainId::parse_caip2(&body.home_chain)
        .map_err(|e| ApiError::InvalidIntent(format!("invalid home_chain: {e}")))?;

    let bond_amount = body
        .bond_amount
        .parse::<Decimal>()
        .map_err(|_| ApiError::InvalidAmount("bond_amount must be a decimal".to_string()))?;
    if bond_amount < Decimal::ZERO {
        return Err(ApiError::InvalidAmount(
            "bond_amount must not be negative".to_string(),
        ));
    }
    if body.name.trim().is_empty() || body.bond_address.trim().is_empty() {
        return Err(ApiError::Validation(
            "name and bond_address must not be empty".to_string(),
        ));
    }

    let solver = state
        .ofa_store
        .create_solver(&NewSolver {
            solver_id: Uuid::new_v4(),
            name: body.name.clone(),
            home_chain: body.home_chain.clone(),
            bond_address: body.bond_address.clone(),
            bond_asset: body.bond_asset.clone(),
            bond_amount: body.bond_amount.clone(),
            api_key_hash: hash_api_key(&body.api_key),
        })
        .await?;

    let status = solver.status();
    Ok(Json(ApiResponse::with_version(
        2,
        SolverResponse {
            solver_id: solver.solver_id,
            name: solver.name,
            home_chain: solver.home_chain,
            bond_address: solver.bond_address,
            bond_asset: solver.bond_asset,
            bond_amount: solver.bond_amount,
            status,
            registered_at_ms: to_ms(solver.registered_at),
            last_heartbeat_at_ms: solver.last_heartbeat_at.map(to_ms),
        },
        request_id.as_str(),
    )))
}

/// `GET /api/v2/solvers/{solver_id}`
#[utoipa::path(
    get,
    path = "/api/v2/solvers/{solver_id}",
    tag = "ofa",
    params(("solver_id" = Uuid, Path, description = "Solver UUID")),
    responses(
        (status = 200, body = ApiResponse<SolverResponse>),
        (status = 404, description = "solver_not_found"),
    ),
)]
pub async fn get_solver(
    State(state): State<Arc<AppState>>,
    request_id: RequestId,
    Path(solver_id): Path<Uuid>,
) -> Result<Json<ApiResponse<SolverResponse>>> {
    let solver = state
        .ofa_store
        .get_solver(solver_id)
        .await?
        .ok_or_else(|| ApiError::SolverNotFound {
            solver_id: solver_id.to_string(),
        })?;

    let status = solver.status();
    Ok(Json(ApiResponse::with_version(
        2,
        SolverResponse {
            solver_id: solver.solver_id,
            name: solver.name,
            home_chain: solver.home_chain,
            bond_address: solver.bond_address,
            bond_asset: solver.bond_asset,
            bond_amount: solver.bond_amount,
            status,
            registered_at_ms: to_ms(solver.registered_at),
            last_heartbeat_at_ms: solver.last_heartbeat_at.map(to_ms),
        },
        request_id.as_str(),
    )))
}

/// Authenticate a solver from `x-solver-id` + `x-solver-key` headers.
async fn authed_solver(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<crate::ofa::store::SolverRow> {
    let raw_id = headers
        .get(SOLVER_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .ok_or(ApiError::SolverKeyMismatch)?;
    let raw_key = headers
        .get(SOLVER_KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .ok_or(ApiError::SolverKeyMismatch)?;
    let solver_id = Uuid::parse_str(raw_id.trim()).map_err(|_| ApiError::SolverKeyMismatch)?;
    let solver = state
        .ofa_store
        .get_solver(solver_id)
        .await?
        .ok_or_else(|| ApiError::SolverKeyMismatch)?;

    if solver.api_key_hash != hash_api_key(raw_key.trim()) {
        return Err(ApiError::SolverKeyMismatch);
    }
    match solver.status() {
        crate::models::SolverStatus::Active => {}
        other => {
            return Err(ApiError::SolverNotActive {
                solver_id: solver_id.to_string(),
                status: other.as_str().to_string(),
            })
        }
    }
    Ok(solver)
}

fn solver_quote_view(
    quote: &crate::ofa::store::QuoteRow,
    solver_name: &str,
) -> SolverQuoteResponse {
    SolverQuoteResponse {
        quote_id: quote.quote_id,
        round_id: quote.round_id,
        solver_id: quote.solver_id,
        solver_name: solver_name.to_string(),
        fill_amount: quote.fill_amount.clone(),
        is_winner: quote.is_winner,
        received_at_ms: to_ms(quote.received_at),
    }
}

/// `POST /api/v2/auctions/{round_id}/quotes`
#[utoipa::path(
    post,
    path = "/api/v2/auctions/{round_id}/quotes",
    tag = "ofa",
    params(
        ("round_id" = Uuid, Path, description = "Auction round UUID"),
        ("x-solver-id" = String, Header, description = "Solver UUID"),
        ("x-solver-key" = String, Header, description = "Solver API key"),
    ),
    request_body = SubmitQuoteRequest,
    responses(
        (status = 200, body = ApiResponse<SolverQuoteResponse>),
        (status = 401, description = "solver_key_mismatch"),
        (status = 404, description = "auction_not_found"),
        (status = 422, description = "auction_closed | quote_below_minimum"),
    ),
)]
pub async fn submit_quote(
    State(state): State<Arc<AppState>>,
    request_id: RequestId,
    headers: HeaderMap,
    Path(round_id): Path<Uuid>,
    Json(body): Json<SubmitQuoteRequest>,
) -> Result<Json<ApiResponse<SolverQuoteResponse>>> {
    let solver = authed_solver(&state, &headers).await?;

    let round = state
        .ofa_store
        .get_round(round_id)
        .await?
        .ok_or_else(|| ApiError::AuctionNotFound {
            round_id: round_id.to_string(),
        })?;

    let now = Utc::now();
    if round.status() != RoundStatus::Open || now >= round.closes_at {
        return Err(ApiError::AuctionClosed {
            round_id: round_id.to_string(),
            status: round.status,
        });
    }

    let fill = body
        .fill_amount
        .parse::<Decimal>()
        .map_err(|_| ApiError::InvalidAmount("fill_amount must be a decimal".to_string()))?;
    if fill <= Decimal::ZERO {
        return Err(ApiError::InvalidAmount(
            "fill_amount must be positive".to_string(),
        ));
    }

    let intent = state
        .ofa_store
        .get_intent(round.intent_id)
        .await?
        .ok_or_else(|| ApiError::IntentNotFound {
            intent_id: round.intent_id.to_string(),
        })?;

    let floor = intent
        .min_output
        .parse::<Decimal>()
        .map_err(|_| ApiError::InvalidIntent("intent min_output is malformed".to_string()))?;
    if fill < floor {
        return Err(ApiError::QuoteBelowMinimum);
    }

    let quote = state
        .ofa_store
        .insert_quote(&NewQuote {
            quote_id: Uuid::new_v4(),
            round_id,
            intent_id: round.intent_id,
            solver_id: solver.solver_id,
            fill_amount: body.fill_amount.clone(),
            execution_path: body.execution_path.clone(),
            quote_signature: body.quote_signature.clone(),
        })
        .await?;

    Ok(Json(ApiResponse::with_version(
        2,
        solver_quote_view(&quote, &solver.name),
        request_id.as_str(),
    )))
}

/// `GET /api/v2/auctions/{round_id}/quotes` — winner-visible status.
#[utoipa::path(
    get,
    path = "/api/v2/auctions/{round_id}/quotes",
    tag = "ofa",
    params(("round_id" = Uuid, Path, description = "Auction round UUID")),
    responses(
        (status = 200, body = ApiResponse<AuctionStatusResponse>),
        (status = 404, description = "auction_not_found"),
    ),
)]
pub async fn auction_status(
    State(state): State<Arc<AppState>>,
    request_id: RequestId,
    Path(round_id): Path<Uuid>,
) -> Result<Json<ApiResponse<AuctionStatusResponse>>> {
    let round = state
        .ofa_store
        .get_round(round_id)
        .await?
        .ok_or_else(|| ApiError::AuctionNotFound {
            round_id: round_id.to_string(),
        })?;

    let quotes = state.ofa_store.list_round_quotes(round_id).await?;
    let winner = if let Some(winner_quote_id) = round.winner_quote_id {
        quotes
            .iter()
            .find(|q| q.quote_id == winner_quote_id)
            .map(|q| {
                // Resolve solver name for the public surface.
                q.clone()
            })
    } else {
        None
    };

    let winner_view = if let Some(w) = winner {
        let solver = state.ofa_store.get_solver(w.solver_id).await?;
        Some(solver_quote_view(
            &w,
            solver.as_ref().map(|s| s.name.as_str()).unwrap_or("unknown"),
        ))
    } else {
        None
    };

    Ok(Json(ApiResponse::with_version(
        2,
        AuctionStatusResponse {
            round_id: round.round_id,
            intent_id: round.intent_id,
            status: round.status(),
            opens_at_ms: to_ms(round.opened_at),
            closes_at_ms: to_ms(round.closes_at),
            winner: winner_view,
            quote_count: quotes.len(),
            view: BEST_QUOTE_PUBLIC_VIEW,
        },
        request_id.as_str(),
    )))
}

/// `POST /api/v2/auctions/{round_id}/settle`
///
/// The winner (or an operator) reports the settlement transaction. The round
/// transitions to `settled` and the intent to `filled`. An unreported winner
/// is slashed by a separate on-chain watcher (see
/// `crates/contracts/src/solver_registry.rs`).
#[utoipa::path(
    post,
    path = "/api/v2/auctions/{round_id}/settle",
    tag = "ofa",
    params(
        ("round_id" = Uuid, Path, description = "Auction round UUID"),
        ("x-solver-id" = String, Header, description = "Solver UUID of the winner"),
        ("x-solver-key" = String, Header, description = "Winner solver API key"),
    ),
    request_body = SettleAuctionRequest,
    responses(
        (status = 200, body = ApiResponse<SettleAuctionResponse>),
        (status = 401, description = "solver_key_mismatch"),
        (status = 404, description = "auction_not_found"),
    ),
)]
pub async fn settle_auction(
    State(state): State<Arc<AppState>>,
    request_id: RequestId,
    headers: HeaderMap,
    Path(round_id): Path<Uuid>,
    Json(body): Json<SettleAuctionRequest>,
) -> Result<Json<ApiResponse<SettleAuctionResponse>>> {
    let solver = authed_solver(&state, &headers).await?;
    let round = state
        .ofa_store
        .get_round(round_id)
        .await?
        .ok_or_else(|| ApiError::AuctionNotFound {
            round_id: round_id.to_string(),
        })?;

    if let Some(winner) = round.winner_solver_id {
        if winner != solver.solver_id {
            return Err(ApiError::Unauthorized(
                "only the winning solver may report settlement".to_string(),
            ));
        }
    }

    let settled = state
        .ofa_store
        .mark_round_settled(round_id, round.intent_id, &body.tx_hash, &body.chain)
        .await?
        .ok_or_else(|| ApiError::AuctionClosed {
            round_id: round_id.to_string(),
            status: round.status,
        })?;

    Ok(Json(ApiResponse::with_version(
        2,
        SettleAuctionResponse {
            round_id,
            intent_id: settled.intent_id,
            status: settled.status(),
            settled_at_ms: settled.settled_at.map(to_ms).unwrap_or(0),
            tx_hash: settled.settled_tx_hash,
        },
        request_id.as_str(),
    )))
}