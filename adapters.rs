use crate::{errors::ContractError, types::Asset};
use soroban_sdk::{contractclient, symbol_short, vec, Address, Env, IntoVal, Symbol};

#[contractclient(name = "PoolAdapterClient")]
pub trait PoolAdapterTrait {
    fn swap(
        e: Env,
        input_asset: Asset,
        output_asset: Asset,
        amount_in: i128,
        min_out: i128,
    ) -> i128;

    fn adapter_quote(e: Env, input_asset: Asset, output_asset: Asset, amount_in: i128) -> i128;

    fn get_rsrvs(e: Env) -> (i128, i128);
}

pub struct AmmAdapter;

impl AmmAdapter {
    pub fn quote(
        e: &Env,
        pool: &Address,
        input_asset: &Asset,
        output_asset: &Asset,
        amount_in: i128,
    ) -> Result<i128, ContractError> {
        let call_result = e.try_invoke_contract::<i128, soroban_sdk::Error>(
            pool,
            &Symbol::new(e, "adapter_quote"),
            vec![
                e,
                input_asset.clone().into_val(e),
                output_asset.clone().into_val(e),
                amount_in.into_val(e),
            ],
        );

        match call_result {
            Ok(Ok(val)) => Ok(val),
            _ => Err(ContractError::AmmQuoteCallFailed),
        }
    }

    pub fn swap(
        e: &Env,
        pool: &Address,
        input_asset: &Asset,
        output_asset: &Asset,
        amount_in: i128,
        min_out: i128,
    ) -> Result<i128, ContractError> {
        let call_result = e.try_invoke_contract::<i128, soroban_sdk::Error>(
            pool,
            &symbol_short!("swap"),
            vec![
                e,
                input_asset.clone().into_val(e),
                output_asset.clone().into_val(e),
                amount_in.into_val(e),
                min_out.into_val(e),
            ],
        );

        match call_result {
            Ok(Ok(val)) => Ok(val),
            _ => Err(ContractError::AmmSwapCallFailed),
        }
    }

    pub fn get_reserves(e: &Env, pool: &Address) -> Result<(i128, i128), ContractError> {
        let reserves_result = e.try_invoke_contract::<(i128, i128), soroban_sdk::Error>(
            pool,
            &symbol_short!("get_rsrvs"),
            vec![e],
        );

        match reserves_result {
            Ok(Ok(val)) => Ok(val),
            _ => Err(ContractError::AmmReservesCallFailed),
        }
    }
}
