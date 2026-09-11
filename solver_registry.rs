//! Stellar-Native Solver Registry for Order Flow Auctions (OFA).
//!
//! This contract manages the on-chain registration of solvers who compete in
//! off-chain auctions to fill user intents. Solvers must post a bond (XLM or
//! other Soroban token) which is slashed if they win an auction but fail to
//! settle on-chain before the deadline.
//!
//! The contract mirrors the Solana Anchor `solver-registry` program so the
//! same economic guarantees apply cross-chain.

use crate::errors::ContractError;
use crate::events;
use crate::storage::{extend_instance_ttl, extend_persistent_ttl, AUCTION_TTL_EXTEND_TO, AUCTION_TTL_THRESHOLD, SOLVER_TTL_EXTEND_TO, SOLVER_TTL_THRESHOLD, StorageKey};
use crate::types::{Asset, AuctionRound, AuctionStatus, SolverInfo, SolverQuote, SolverRegistryConfig, SolverStatus};
use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, Symbol};

#[contract]
pub struct SolverRegistry;

#[contractimpl]
impl SolverRegistry {
    /// Initialize the solver registry.
    ///
    /// # Arguments
    /// * `admin` - Admin address (can suspend/slash solvers, update config)
    /// * `treasury` - Address receiving slashed bonds
    /// * `native_token` - Native XLM token contract address
    /// * `min_bond_amount` - Minimum bond required to register (in bond asset units)
    /// * `auction_window_secs` - Off-chain auction window (2–3 seconds, typical 2.5s)
    /// * `slash_percentage_bps` - Percentage of bond to slash on settlement failure (basis points)
    pub fn initialize(
        e: Env,
        admin: Address,
        treasury: Address,
        native_token: Address,
        min_bond_amount: i128,
        auction_window_secs: u32,
        slash_percentage_bps: u32,
    ) -> Result<(), ContractError> {
        if e.storage().instance().has(&StorageKey::SolverRegistryConfig) {
            return Err(ContractError::AlreadyInitialized);
        }

        let config = SolverRegistryConfig {
            min_bond_amount,
            auction_window_secs,
            slash_percentage_bps,
            admin: admin.clone(),
            treasury: treasury.clone(),
            native_token,
        };

        e.storage().instance().set(&StorageKey::SolverRegistryConfig, &config);
        e.storage().instance().set(&StorageKey::SolverCount, &0u32);

        events::solver_registered(&e, admin, Asset::Native, 0); // reusing event for init log
        extend_instance_ttl(&e);
        Ok(())
    }

    /// Register a new solver with a bonded stake.
    ///
    /// The solver must transfer the bond amount to this contract before calling.
    /// For native XLM, the bond is sent via `transfer_native`. For Soroban tokens,
    /// the bond is transferred via the token contract's `transfer`.
    ///
    /// # Arguments
    /// * `solver` - Solver's address (must authorize)
    /// * `name` - Human-readable solver name
    /// * `bond_asset` - Asset used for the bond (Native XLM or Soroban token)
    /// * `bond_amount` - Amount to stake (must be >= min_bond_amount)
    pub fn register_solver(
        e: Env,
        solver: Address,
        name: Symbol,
        bond_asset: Asset,
        bond_amount: i128,
    ) -> Result<(), ContractError> {
        solver.require_auth();

        let config: SolverRegistryConfig = e
            .storage()
            .instance()
            .get(&StorageKey::SolverRegistryConfig)
            .ok_or(ContractError::NotInitialized)?;

        if bond_amount < config.min_bond_amount {
            return Err(ContractError::InvalidAmount);
        }

        // Check solver not already registered
        let solver_key = StorageKey::Solver(solver.clone());
        if e.storage().persistent().has(&solver_key) {
            return Err(ContractError::AlreadyInitialized);
        }

        // Transfer bond from solver to this contract
        Self::transfer_bond(&e, &solver, &bond_asset, bond_amount)?;

        let solver_info = SolverInfo {
            address: solver.clone(),
            name,
            bond_asset: bond_asset.clone(),
            bond_amount,
            status: SolverStatus::Active,
            registered_at: e.ledger().sequence() as u64,
            last_heartbeat_at: e.ledger().sequence() as u64,
        };

        e.storage().persistent().set(&solver_key, &solver_info);
        extend_persistent_ttl(&e, &solver_key, SOLVER_TTL_THRESHOLD, SOLVER_TTL_EXTEND_TO);

        let mut count: u32 = e
            .storage()
            .instance()
            .get(&StorageKey::SolverCount)
            .unwrap_or(0);
        count += 1;
        e.storage().instance().set(&StorageKey::SolverCount, &count);

        events::solver_registered(&e, solver, bond_asset, bond_amount);
        extend_instance_ttl(&e);
        Ok(())
    }

    /// Unregister a solver and return their bond (admin only).
    ///
    /// The solver must not have any pending auctions.
    pub fn unregister_solver(e: Env, admin: Address, solver: Address) -> Result<(), ContractError> {
        Self::require_admin(&e, &admin)?;

        let solver_key = StorageKey::Solver(solver.clone());
        let solver_info: SolverInfo = e
            .storage()
            .persistent()
            .get(&solver_key)
            .ok_or(ContractError::InvalidRecipient)?;

        // Return bond to solver
        Self::return_bond(&e, &solver_info)?;

        e.storage().persistent().remove(&solver_key);

        let mut count: u32 = e
            .storage()
            .instance()
            .get(&StorageKey::SolverCount)
            .unwrap_or(1);
        count -= 1;
        e.storage().instance().set(&StorageKey::SolverCount, &count);

        events::solver_unregistered(&e, solver, admin);
        extend_instance_ttl(&e);
        Ok(())
    }

    /// Suspend a solver (admin only). Suspended solvers cannot submit quotes.
    pub fn suspend_solver(e: Env, admin: Address, solver: Address) -> Result<(), ContractError> {
        Self::require_admin(&e, &admin)?;
        Self::update_solver_status(&e, solver.clone(), SolverStatus::Suspended)?;
        events::solver_suspended(&e, solver, admin);
        extend_instance_ttl(&e);
        Ok(())
    }

    /// Reactivate a suspended solver (admin only).
    pub fn activate_solver(e: Env, admin: Address, solver: Address) -> Result<(), ContractError> {
        Self::require_admin(&e, &admin)?;
        Self::update_solver_status(&e, solver.clone(), SolverStatus::Active)?;
        events::solver_activated(&e, solver, admin);
        extend_instance_ttl(&e);
        Ok(())
    }

    /// Open a new auction round (called by off-chain coordinator via admin).
    ///
    /// # Arguments
    /// * `admin` - Must be registry admin
    /// * `auction_id` - 32-byte auction identifier (from off-chain coordinator)
    /// * `intent_hash` - Hash of the user intent (32 bytes)
    /// * `deadline` - Unix timestamp when auction closes and settlement must complete
    /// * `min_output` - Minimum output amount the user will accept
    pub fn open_auction(
        e: Env,
        admin: Address,
        auction_id: BytesN<32>,
        intent_hash: BytesN<32>,
        deadline: u64,
        min_output: i128,
    ) -> Result<(), ContractError> {
        Self::require_admin(&e, &admin)?;

        if e.storage().persistent().has(&StorageKey::Auction(auction_id.clone())) {
            return Err(ContractError::AlreadyInitialized);
        }

        let round = AuctionRound {
            auction_id: auction_id.clone(),
            intent_hash: intent_hash.clone(),
            deadline,
            min_output,
            status: AuctionStatus::Open,
            winner: None,
            winner_fill_amount: 0,
            opened_at: e.ledger().sequence() as u64,
            closed_at: 0,
            settled_tx_hash: BytesN::from_array(&e, &[0u8; 32]),
        };

        e.storage().persistent().set(&StorageKey::Auction(auction_id.clone()), &round);
        extend_persistent_ttl(&e, &StorageKey::Auction(auction_id.clone()), AUCTION_TTL_THRESHOLD, AUCTION_TTL_EXTEND_TO);

        events::auction_opened(&e, auction_id, intent_hash, deadline, min_output);
        extend_instance_ttl(&e);
        Ok(())
    }

    /// Submit a quote for an open auction.
    ///
    /// # Arguments
    /// * `solver` - Solver address (must authorize, must be active)
    /// * `auction_id` - Auction to bid on
    /// * `fill_amount` - Amount of destination asset the solver commits to deliver
    pub fn submit_quote(
        e: Env,
        solver: Address,
        auction_id: BytesN<32>,
        fill_amount: i128,
    ) -> Result<(), ContractError> {
        solver.require_auth();

        let mut round: AuctionRound = e
            .storage()
            .persistent()
            .get(&StorageKey::Auction(auction_id.clone()))
            .ok_or(ContractError::InvalidRoute)?;

        if round.status != AuctionStatus::Open {
            return Err(ContractError::InvalidRoute);
        }
        if (e.ledger().sequence() as u64) > round.deadline {
            round.status = AuctionStatus::ExpiredNoWinner;
            e.storage().persistent().set(&StorageKey::Auction(auction_id.clone()), &round);
            return Err(ContractError::InvalidRoute);
        }

        // Verify solver is active
let solver_key = StorageKey::Solver(solver.clone());
        let solver_info: SolverInfo = e
            .storage()
            .persistent()
            .get(&solver_key)
            .ok_or(ContractError::InvalidRecipient)?;

        if solver_info.status != SolverStatus::Active {
            return Err(ContractError::Unauthorized);
        }

        if fill_amount < round.min_output {
            return Err(ContractError::InvalidRoute);
        }

        let quote = SolverQuote {
            solver: solver.clone(),
            fill_amount,
            submitted_at: e.ledger().sequence() as u64,
        };

        e.storage()
            .persistent()
            .set(&StorageKey::AuctionQuote(auction_id.clone(), solver.clone()), &quote);
        extend_persistent_ttl(&e, &StorageKey::AuctionQuote(auction_id.clone(), solver.clone()), AUCTION_TTL_THRESHOLD, AUCTION_TTL_EXTEND_TO);

        events::quote_submitted(&e, auction_id, solver, fill_amount);
        extend_instance_ttl(&e);
        Ok(())
    }

    /// Close auction and select winner (admin/coordinator call).
    ///
    /// Selects the highest fill_amount quote that meets min_output.
    /// Ties broken by earliest submission.
    pub fn close_auction(e: Env, admin: Address, auction_id: BytesN<32>) -> Result<(), ContractError> {
        Self::require_admin(&e, &admin)?;

        let mut round: AuctionRound = e
            .storage()
            .persistent()
            .get(&StorageKey::Auction(auction_id.clone()))
            .ok_or(ContractError::InvalidRoute)?;

        if round.status != AuctionStatus::Open {
            return Err(ContractError::InvalidRoute);
        }

        // Winner selection happens off-chain (trusted coordinator) and is
        // recorded on-chain via select_winner. Closing just freezes bidding.
        round.status = AuctionStatus::Closed;
        round.closed_at = e.ledger().sequence() as u64;
        e.storage().persistent().set(&StorageKey::Auction(auction_id.clone()), &round);

        events::auction_closed(&e, auction_id);
        extend_instance_ttl(&e);
        Ok(())
    }

    /// Select the winning quote (called by trusted coordinator/admin).
    ///
    /// # Arguments
    /// * `admin` - Must be registry admin
    /// * `auction_id` - Auction to resolve
    /// * `winner` - Winning solver address
    /// * `fill_amount` - Winning fill amount
    pub fn select_winner(
        e: Env,
        admin: Address,
        auction_id: BytesN<32>,
        winner: Address,
        fill_amount: i128,
    ) -> Result<(), ContractError> {
        Self::require_admin(&e, &admin)?;

        let mut round: AuctionRound = e
            .storage()
            .persistent()
            .get(&StorageKey::Auction(auction_id.clone()))
            .ok_or(ContractError::InvalidRoute)?;

        if round.status != AuctionStatus::Open && round.status != AuctionStatus::Closed {
            return Err(ContractError::InvalidRoute);
        }

        // Verify the quote exists and matches
        let quote: SolverQuote = e
            .storage()
            .persistent()
            .get(&StorageKey::AuctionQuote(auction_id.clone(), winner.clone()))
            .ok_or(ContractError::InvalidRoute)?;

        if quote.fill_amount != fill_amount {
            return Err(ContractError::InvalidRoute);
        }
        if fill_amount < round.min_output {
            return Err(ContractError::InvalidRoute);
        }

        round.status = AuctionStatus::Closed;
        round.winner = Some(winner.clone());
        round.winner_fill_amount = fill_amount;
        e.storage().persistent().set(&StorageKey::Auction(auction_id.clone()), &round);
        e.storage().instance().set(&StorageKey::AuctionWinner(auction_id.clone()), &winner);

        events::winner_selected(&e, auction_id, winner, fill_amount);
        extend_instance_ttl(&e);
        Ok(())
    }

    /// Report successful settlement (winner or admin).
    ///
    /// Called after the winner executes the fill on-chain.
    /// The winner must provide the settlement transaction hash.
    pub fn settle_auction(
        e: Env,
        caller: Address,
        auction_id: BytesN<32>,
        tx_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        let mut round: AuctionRound = e
            .storage()
            .persistent()
            .get(&StorageKey::Auction(auction_id.clone()))
            .ok_or(ContractError::InvalidRoute)?;

        let winner = round.winner.clone().ok_or(ContractError::InvalidRoute)?;
        let config: SolverRegistryConfig = e
            .storage()
            .instance()
            .get(&StorageKey::SolverRegistryConfig)
            .ok_or(ContractError::NotInitialized)?;
        if caller != winner && caller != config.admin {
            return Err(ContractError::Unauthorized);
        }
        if round.status == AuctionStatus::Settled {
            return Err(ContractError::InvalidRoute);
        }

        round.status = AuctionStatus::Settled;
        round.settled_tx_hash = tx_hash.clone();
        e.storage().persistent().set(&StorageKey::Auction(auction_id.clone()), &round);

        events::auction_settled(&e, auction_id, winner, tx_hash);
        extend_instance_ttl(&e);
        Ok(())
    }

    /// Slash a solver's bond for failing to settle after winning.
    ///
    /// Can be called by anyone after the auction deadline passes without settlement.
    /// The slashed amount (slash_percentage_bps of bond) goes to treasury.
    pub fn slash_solver(e: Env, auction_id: BytesN<32>) -> Result<(), ContractError> {
        let round: AuctionRound = e
            .storage()
            .persistent()
            .get(&StorageKey::Auction(auction_id.clone()))
            .ok_or(ContractError::InvalidRoute)?;

        if round.status != AuctionStatus::Closed {
            return Err(ContractError::InvalidRoute);
        }
        if (e.ledger().sequence() as u64) <= round.deadline {
            return Err(ContractError::InvalidRoute); // Deadline not passed yet
        }

        let winner = round.winner.clone().ok_or(ContractError::InvalidRoute)?;
        let solver_key = StorageKey::Solver(winner.clone());
        let mut solver_info: SolverInfo = e
            .storage()
            .persistent()
            .get(&solver_key)
            .ok_or(ContractError::InvalidRecipient)?;

        let config: SolverRegistryConfig = e
            .storage()
            .instance()
            .get(&StorageKey::SolverRegistryConfig)
            .ok_or(ContractError::NotInitialized)?;

        // Calculate slash amount
        let slash_amount = (solver_info.bond_amount * config.slash_percentage_bps as i128) / 10000;
        if slash_amount > solver_info.bond_amount {
            return Err(ContractError::InvalidAmount);
        }

        // Transfer slashed amount to treasury
        Self::transfer_bond(&e, &winner, &solver_info.bond_asset, slash_amount)?;

        // Update solver bond
        solver_info.bond_amount -= slash_amount;
        if solver_info.bond_amount < config.min_bond_amount {
            solver_info.status = SolverStatus::Slashed;
        }
        e.storage().persistent().set(&solver_key, &solver_info);

        events::solver_slashed(&e, winner, auction_id, slash_amount);
        extend_instance_ttl(&e);
        Ok(())
    }

    /// Update solver heartbeat (called by solver to show liveness).
    pub fn heartbeat(e: Env, solver: Address) -> Result<(), ContractError> {
        solver.require_auth();

        let solver_key = StorageKey::Solver(solver.clone());
        let mut solver_info: SolverInfo = e
            .storage()
            .persistent()
            .get(&solver_key)
            .ok_or(ContractError::InvalidRecipient)?;

        solver_info.last_heartbeat_at = e.ledger().sequence() as u64;
        e.storage().persistent().set(&solver_key, &solver_info);
        extend_persistent_ttl(&e, &solver_key, SOLVER_TTL_THRESHOLD, SOLVER_TTL_EXTEND_TO);
        Ok(())
    }

    /// Get solver info.
    pub fn get_solver(e: Env, solver: Address) -> Result<SolverInfo, ContractError> {
        let solver_key = StorageKey::Solver(solver);
        e.storage()
            .persistent()
            .get(&solver_key)
            .ok_or(ContractError::InvalidRecipient)
    }

    /// Get auction round info.
    pub fn get_auction(e: Env, auction_id: BytesN<32>) -> Result<AuctionRound, ContractError> {
        e.storage()
            .persistent()
            .get(&StorageKey::Auction(auction_id))
            .ok_or(ContractError::InvalidRoute)
    }

    /// Get solver quote for an auction.
    pub fn get_quote(e: Env, auction_id: BytesN<32>, solver: Address) -> Result<SolverQuote, ContractError> {
        e.storage()
            .persistent()
            .get(&StorageKey::AuctionQuote(auction_id, solver))
            .ok_or(ContractError::InvalidRecipient)
    }

    /// Update registry config (admin only).
    pub fn update_config(
        e: Env,
        admin: Address,
        min_bond_amount: Option<i128>,
        auction_window_secs: Option<u32>,
        slash_percentage_bps: Option<u32>,
        treasury: Option<Address>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&e, &admin)?;

        let mut config: SolverRegistryConfig = e
            .storage()
            .instance()
            .get(&StorageKey::SolverRegistryConfig)
            .ok_or(ContractError::NotInitialized)?;

        if let Some(v) = min_bond_amount {
            config.min_bond_amount = v;
        }
        if let Some(v) = auction_window_secs {
            config.auction_window_secs = v;
        }
        if let Some(v) = slash_percentage_bps {
            config.slash_percentage_bps = v;
        }
        if let Some(v) = treasury {
            config.treasury = v;
        }

        e.storage().instance().set(&StorageKey::SolverRegistryConfig, &config);
        extend_instance_ttl(&e);
        Ok(())
    }

    /// Get registry config.
    pub fn get_config(e: Env) -> Result<SolverRegistryConfig, ContractError> {
        e.storage()
            .instance()
            .get(&StorageKey::SolverRegistryConfig)
            .ok_or(ContractError::NotInitialized)
    }

    // ── Internal helpers ────────────────────────────────────────────────

    fn require_admin(e: &Env, admin: &Address) -> Result<(), ContractError> {
        let config: SolverRegistryConfig = e
            .storage()
            .instance()
            .get(&StorageKey::SolverRegistryConfig)
            .ok_or(ContractError::NotInitialized)?;
        if admin != &config.admin {
            return Err(ContractError::Unauthorized);
        }
        admin.require_auth();
        Ok(())
    }

    fn update_solver_status(e: &Env, solver: Address, status: SolverStatus) -> Result<(), ContractError> {
        let solver_key = StorageKey::Solver(solver.clone());
        let mut info: SolverInfo = e
            .storage()
            .persistent()
            .get(&solver_key)
            .ok_or(ContractError::InvalidRecipient)?;
        info.status = status;
        e.storage().persistent().set(&solver_key, &info);
        extend_persistent_ttl(e, &solver_key, SOLVER_TTL_THRESHOLD, SOLVER_TTL_EXTEND_TO);
        Ok(())
    }

    fn transfer_bond(e: &Env, from: &Address, asset: &Asset, amount: i128) -> Result<(), ContractError> {
        let config: SolverRegistryConfig = e
            .storage()
            .instance()
            .get(&StorageKey::SolverRegistryConfig)
            .ok_or(ContractError::NotInitialized)?;

        match asset {
            Asset::Native => {
                let client = soroban_sdk::token::Client::new(e, &config.native_token);
                client.transfer(from, &e.current_contract_address(), &amount);
            }
            Asset::Soroban(addr) => {
                let client = soroban_sdk::token::Client::new(e, addr);
                client.transfer(from, &e.current_contract_address(), &amount);
            }
            _ => return Err(ContractError::InvalidAmount),
        }
        Ok(())
    }

    fn return_bond(e: &Env, solver_info: &SolverInfo) -> Result<(), ContractError> {
        let config: SolverRegistryConfig = e
            .storage()
            .instance()
            .get(&StorageKey::SolverRegistryConfig)
            .ok_or(ContractError::NotInitialized)?;

        match &solver_info.bond_asset {
            Asset::Native => {
                let client = soroban_sdk::token::Client::new(e, &config.native_token);
                client.transfer(&e.current_contract_address(), &solver_info.address, &solver_info.bond_amount);
            }
            Asset::Soroban(addr) => {
                let client = soroban_sdk::token::Client::new(e, addr);
                client.transfer(&e.current_contract_address(), &solver_info.address, &solver_info.bond_amount);
            }
            _ => return Err(ContractError::InvalidAmount),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        testutils::Address as _,
        Address, BytesN, Env, Symbol,
    };

    fn setup(e: &Env) -> (SolverRegistryClient<'_>, Address, Address, Address) {
        e.mock_all_auths();
        let admin = Address::generate(e);
        let treasury = Address::generate(e);
        let solver = Address::generate(e);
        // Deploy native token for testing
        let native_token = e.register_stellar_asset_contract_v2(admin.clone());
        // Fund the solver so bond transfers succeed
        let token = soroban_sdk::token::StellarAssetClient::new(e, &native_token.address());
        token.mint(&solver, &1_000_000_000_000);
        let contract_id = e.register_contract(None, SolverRegistry);
        let client = SolverRegistryClient::new(e, &contract_id);
        client.initialize(
            &admin,
            &treasury,
            &native_token.address(),
            &10000000,
            &2,
            &1000, // 10 XLM min, 2s window, 10% slash
        );
        (client, admin, treasury, solver)
    }

    #[test]
    fn register_and_get_solver() {
        let e = Env::default();
        let (client, _admin, _treasury, solver) = setup(&e);

        client.register_solver(&solver, &Symbol::new(&e, "test_solver"), &Asset::Native, &20000000);

        let info = client.get_solver(&solver);
        assert_eq!(info.address, solver);
        assert_eq!(info.name, Symbol::new(&e, "test_solver"));
        assert_eq!(info.bond_amount, 20000000);
        assert_eq!(info.status, SolverStatus::Active);
    }

    #[test]
    fn register_insufficient_bond_fails() {
        let e = Env::default();
        let (client, _admin, _treasury, solver) = setup(&e);

        let result = client.try_register_solver(&solver, &Symbol::new(&e, "poor_solver"), &Asset::Native, &5000000);
        assert!(result.is_err());
    }

    #[test]
    fn suspend_and_activate() {
        let e = Env::default();
        let (client, admin, _treasury, solver) = setup(&e);

        client.register_solver(&solver, &Symbol::new(&e, "solver"), &Asset::Native, &20000000);
        client.suspend_solver(&admin, &solver);

        let info = client.get_solver(&solver);
        assert_eq!(info.status, SolverStatus::Suspended);

        client.activate_solver(&admin, &solver);
        let info = client.get_solver(&solver);
        assert_eq!(info.status, SolverStatus::Active);
    }

    #[test]
    fn open_auction_submit_quote() {
        let e = Env::default();
        let (client, admin, _treasury, solver) = setup(&e);

        client.register_solver(&solver, &Symbol::new(&e, "solver"), &Asset::Native, &20000000);

        let auction_id = BytesN::from_array(&e, &[1u8; 32]);
        let intent_hash = BytesN::from_array(&e, &[2u8; 32]);
        client.open_auction(&admin, &auction_id, &intent_hash, &1000, &5000000);

        client.submit_quote(&solver, &auction_id, &6000000);

        let quote = client.get_quote(&auction_id, &solver);
        assert_eq!(quote.fill_amount, 6000000);
        assert_eq!(quote.solver, solver);
    }

    #[test]
    fn quote_below_minimum_fails() {
        let e = Env::default();
        let (client, admin, _treasury, solver) = setup(&e);

        client.register_solver(&solver, &Symbol::new(&e, "solver"), &Asset::Native, &20000000);

        let auction_id = BytesN::from_array(&e, &[1u8; 32]);
        let intent_hash = BytesN::from_array(&e, &[2u8; 32]);
        client.open_auction(&admin, &auction_id, &intent_hash, &1000, &5000000);

        let result = client.try_submit_quote(&solver, &auction_id, &4000000);
        assert!(result.is_err());
    }
}