-- Stellar-Native Order Flow Auction (OFA) persistence.
--
-- User submissions are *intents*: signed-but-unbroadcast expressions of
-- "swap X on one chain for Y on another". Solvers register with a staked bond
-- (Stellar Soroban registry or Solana Anchor registry) and compete for the
-- best fill during a short off-chain auction. The winning quote becomes a
-- commitment; settlement must land before an on-chain deadline or the bond is
-- slashed.
--
-- Amounts are stored as decimal strings (matching cctp_transfers and the
-- v1 model convention) — never floating point, never fiat-typed.

CREATE TABLE IF NOT EXISTS ofa_intents (
    intent_id UUID PRIMARY KEY,
    user_address TEXT NOT NULL,
    from_asset TEXT NOT NULL,
    from_asset_canonical TEXT NOT NULL,
    from_chain TEXT NOT NULL,
    from_amount TEXT NOT NULL,
    to_asset TEXT NOT NULL,
    to_asset_canonical TEXT NOT NULL,
    to_chain TEXT NOT NULL,
    to_address TEXT NOT NULL,
    deadline TIMESTAMPTZ NOT NULL,
    min_output TEXT NOT NULL,
    signature TEXT,
    nonce BIGINT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'auction_open', 'auction_won', 'filled', 'abandoned', 'expired', 'cancelled')),
    cancel_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- An intent must not be re-consumed; the persisted status is the source of
-- truth for the coordinator's compare-and-swap.
CREATE UNIQUE INDEX IF NOT EXISTS idx_ofa_intents_user_nonce
    ON ofa_intents (from_chain, user_address, nonce);

CREATE INDEX IF NOT EXISTS idx_ofa_intents_status_deadline
    ON ofa_intents (status, deadline);

CREATE TABLE IF NOT EXISTS ofa_solvers (
    solver_id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    home_chain TEXT NOT NULL,
    bond_address TEXT NOT NULL,
    bond_asset TEXT NOT NULL DEFAULT 'native',
    bond_amount TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'suspended', 'slashed')),
    registered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_heartbeat_at TIMESTAMPTZ,
    api_key_hash TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ofa_solvers_home_chain_status
    ON ofa_solvers (home_chain, status);

CREATE TABLE IF NOT EXISTS ofa_auction_rounds (
    round_id UUID PRIMARY KEY,
    intent_id UUID NOT NULL REFERENCES ofa_intents (intent_id) ON DELETE CASCADE,
    opened_at TIMESTAMPTZ NOT NULL,
    closes_at TIMESTAMPTZ NOT NULL,
    status TEXT NOT NULL DEFAULT 'open'
        CHECK (status IN ('open', 'closed', 'settled', 'expired')),
    winner_solver_id UUID REFERENCES ofa_solvers (solver_id),
    winner_quote_id UUID,
    settled_tx_hash TEXT,
    settled_chain TEXT,
    settled_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ofa_rounds_intent
    ON ofa_auction_rounds (intent_id);

CREATE INDEX IF NOT EXISTS idx_ofa_rounds_status_close
    ON ofa_auction_rounds (status, closes_at);

CREATE TABLE IF NOT EXISTS ofa_solver_quotes (
    quote_id UUID PRIMARY KEY,
    round_id UUID NOT NULL REFERENCES ofa_auction_rounds (round_id) ON DELETE CASCADE,
    intent_id UUID NOT NULL REFERENCES ofa_intents (intent_id) ON DELETE CASCADE,
    solver_id UUID NOT NULL REFERENCES ofa_solvers (solver_id),
    fill_amount TEXT NOT NULL,
    execution_path TEXT,
    quote_signature TEXT NOT NULL,
    is_winner BOOLEAN NOT NULL DEFAULT FALSE,
    received_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- Unique: a winner can only be chosen once per round.
    CONSTRAINT one_winner_per_round UNIQUE (round_id, is_winner)
        DEFERRABLE INITIALLY IMMEDIATE
);

CREATE INDEX IF NOT EXISTS idx_ofa_quotes_round
    ON ofa_solver_quotes (round_id);

CREATE INDEX IF NOT EXISTS idx_ofa_quotes_winner
    ON ofa_solver_quotes (round_id, is_winner);

CREATE TABLE IF NOT EXISTS ofa_slash_events (
    slash_id UUID PRIMARY KEY,
    solver_id UUID NOT NULL REFERENCES ofa_solvers (solver_id),
    round_id UUID NOT NULL REFERENCES ofa_auction_rounds (round_id) ON DELETE CASCADE,
    reason TEXT NOT NULL,
    amount TEXT NOT NULL,
    slash_tx_hash TEXT,
    slashed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ofa_slashes_solver
    ON ofa_slash_events (solver_id);

COMMENT ON TABLE ofa_intents IS
    'Signed-but-unbroadcast swap intents auctioned to bonded solvers.';
COMMENT ON TABLE ofa_solvers IS
    'Solver registrations mirrored from on-chain registry contracts. Bond tracked on-chain; this is a projection.';
COMMENT ON TABLE ofa_solver_quotes IS
    'Off-chain auction bids. Winning quote produces the settlement commitment.';
COMMENT ON TABLE ofa_auction_rounds IS
    'One auction per intent; winner must settle before the intent deadline.';
COMMENT ON TABLE ofa_slash_events IS
    'Bond slashes for picked-but-unsettled auctions (and policy violations).';