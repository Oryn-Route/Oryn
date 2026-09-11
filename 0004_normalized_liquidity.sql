-- OrynRoute - Phase 1.5
-- Unified liquidity surface across SDEX orderbook and AMM reserves

create table if not exists amm_pool_reserves (
  pool_address text primary key,
  selling_asset_id uuid not null references assets(id),
  buying_asset_id uuid not null references assets(id),
  reserve_selling numeric(38, 18) not null check (reserve_selling > 0),
  reserve_buying numeric(38, 18) not null check (reserve_buying > 0),
  fee_bps integer not null default 30 check (fee_bps >= 0 and fee_bps <= 10000),
  last_updated_ledger bigint not null,
  updated_at timestamptz not null default now(),
  check (selling_asset_id != buying_asset_id)
);

create index if not exists idx_amm_pool_reserves_pair
  on amm_pool_reserves (selling_asset_id, buying_asset_id);

create index if not exists idx_amm_pool_reserves_updated
  on amm_pool_reserves (updated_at desc);

create or replace function upsert_amm_pool_reserve(
  p_pool_address text,
  p_selling_asset_id uuid,
  p_buying_asset_id uuid,
  p_reserve_selling numeric,
  p_reserve_buying numeric,
  p_fee_bps integer,
  p_last_updated_ledger bigint
)
returns void as $$
begin
  insert into amm_pool_reserves (
    pool_address,
    selling_asset_id,
    buying_asset_id,
    reserve_selling,
    reserve_buying,
    fee_bps,
    last_updated_ledger,
    updated_at
  )
  values (
    p_pool_address,
    p_selling_asset_id,
    p_buying_asset_id,
    p_reserve_selling,
    p_reserve_buying,
    p_fee_bps,
    p_last_updated_ledger,
    now()
  )
  on conflict (pool_address)
  do update set
    selling_asset_id = excluded.selling_asset_id,
    buying_asset_id = excluded.buying_asset_id,
    reserve_selling = excluded.reserve_selling,
    reserve_buying = excluded.reserve_buying,
    fee_bps = excluded.fee_bps,
    last_updated_ledger = excluded.last_updated_ledger,
    updated_at = now();
end;
$$ language plpgsql;

-- Single query surface used by routing and quote reads.
create or replace view normalized_liquidity as
select
  'sdex'::text as venue_type,
  o.offer_id::text as venue_ref,
  o.selling_asset_id,
  o.buying_asset_id,
  o.price as price,
  o.amount as available_amount,
  o.last_modified_ledger as source_ledger,
  o.updated_at
from sdex_offers o

union all

select
  'amm'::text as venue_type,
  r.pool_address as venue_ref,
  r.selling_asset_id,
  r.buying_asset_id,
  (r.reserve_buying / nullif(r.reserve_selling, 0)) as price,
  r.reserve_selling as available_amount,
  r.last_updated_ledger as source_ledger,
  r.updated_at
from amm_pool_reserves r;

create index if not exists idx_sdex_offers_pair_price_for_normalized
  on sdex_offers (selling_asset_id, buying_asset_id, price asc);

comment on table amm_pool_reserves is 'Latest AMM reserve state used for unified liquidity reads';
comment on view normalized_liquidity is 'Unified SDEX + AMM liquidity surface for routing and quote endpoints';
comment on function upsert_amm_pool_reserve is 'Idempotent AMM reserve writer used by indexer jobs';
