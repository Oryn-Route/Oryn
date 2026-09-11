-- OrynRoute - Phase 1.3
-- Performance indexes and optimizations

-- Add indexes for common query patterns on sdex_offers

-- Index for querying offers by seller
create index if not exists idx_sdex_offers_seller
  on sdex_offers (seller);

-- Index for time-based queries (recent offers)
create index if not exists idx_sdex_offers_ledger
  on sdex_offers (last_modified_ledger desc);

-- Index for updated_at for recent changes
create index if not exists idx_sdex_offers_updated_at
  on sdex_offers (updated_at desc);

-- Composite index for seller + asset pair queries
create index if not exists idx_sdex_offers_seller_pair
  on sdex_offers (seller, selling_asset_id, buying_asset_id);

-- Index for price queries (finding best prices)
create index if not exists idx_sdex_offers_price
  on sdex_offers (selling_asset_id, buying_asset_id, price);

-- Add indexes for assets table

-- Index for asset type lookup
create index if not exists idx_assets_type
  on assets (asset_type);

-- Index for asset code lookup (common for searching specific assets)
create index if not exists idx_assets_code
  on assets (asset_code) where asset_code is not null;

-- Index for asset issuer lookup
create index if not exists idx_assets_issuer
  on assets (asset_issuer) where asset_issuer is not null;

-- Add table for tracking database health metrics
create table if not exists db_health_metrics (
  id uuid primary key default uuid_generate_v4(),
  metric_name text not null,
  metric_value numeric not null,
  metric_unit text null, -- 'count', 'bytes', 'ms', etc.
  metadata jsonb null,
  recorded_at timestamptz not null default now()
);

-- Index for querying recent metrics
create index if not exists idx_db_health_metrics_recorded_at
  on db_health_metrics (recorded_at desc);

-- Index for querying by metric name
create index if not exists idx_db_health_metrics_name
  on db_health_metrics (metric_name, recorded_at desc);

-- Add table for data archival tracking
create table if not exists archived_offers (
  offer_id bigint primary key,
  seller text not null,
  selling_asset_type text not null,
  selling_asset_code text null,
  selling_asset_issuer text null,
  buying_asset_type text not null,
  buying_asset_code text null,
  buying_asset_issuer text null,
  amount numeric(30, 14) not null,
  price numeric(30, 14) not null,
  price_n bigint null,
  price_d bigint null,
  last_modified_ledger bigint not null,
  archived_at timestamptz not null default now(),
  archive_reason text null -- 'expired', 'old', 'replaced', etc.
);

-- Index for archived offers by archive date
create index if not exists idx_archived_offers_archived_at
  on archived_offers (archived_at desc);

-- Index for finding archived offers by seller
create index if not exists idx_archived_offers_seller
  on archived_offers (seller);

-- Add view for active offers (simplified queries)
create or replace view active_offers as
select
  o.offer_id,
  o.seller,
  sa.asset_type as selling_asset_type,
  sa.asset_code as selling_asset_code,
  sa.asset_issuer as selling_asset_issuer,
  ba.asset_type as buying_asset_type,
  ba.asset_code as buying_asset_code,
  ba.asset_issuer as buying_asset_issuer,
  o.amount,
  o.price,
  o.price_n,
  o.price_d,
  o.last_modified_ledger,
  o.updated_at
from sdex_offers o
join assets sa on o.selling_asset_id = sa.id
join assets ba on o.buying_asset_id = ba.id;

-- Add function to archive old offers
create or replace function archive_old_offers(days_old integer default 30)
returns integer as $$
declare
  archived_count integer;
begin
  -- Move offers older than N days to archive table
  with archived as (
    insert into archived_offers (
      offer_id, seller,
      selling_asset_type, selling_asset_code, selling_asset_issuer,
      buying_asset_type, buying_asset_code, buying_asset_issuer,
      amount, price, price_n, price_d,
      last_modified_ledger, archive_reason
    )
    select
      o.offer_id, o.seller,
      sa.asset_type, sa.asset_code, sa.asset_issuer,
      ba.asset_type, ba.asset_code, ba.asset_issuer,
      o.amount, o.price, o.price_n, o.price_d,
      o.last_modified_ledger,
      'old_age'
    from sdex_offers o
    join assets sa on o.selling_asset_id = sa.id
    join assets ba on o.buying_asset_id = ba.id
    where o.updated_at < now() - interval '1 day' * days_old
    returning offer_id
  )
  delete from sdex_offers
  where offer_id in (select offer_id from archived)
  returning * into archived_count;
  
  return coalesce(archived_count, 0);
end;
$$ language plpgsql;

-- Add function to get database health metrics
create or replace function get_db_health_metrics()
returns table (
  metric_name text,
  metric_value numeric,
  metric_unit text
) as $$
begin
  return query
  select
    'total_offers'::text,
    count(*)::numeric,
    'count'::text
  from sdex_offers
  union all
  select
    'total_assets'::text,
    count(*)::numeric,
    'count'::text
  from assets
  union all
  select
    'total_archived_offers'::text,
    count(*)::numeric,
    'count'::text
  from archived_offers
  union all
  select
    'offers_table_size'::text,
    pg_total_relation_size('sdex_offers')::numeric,
    'bytes'::text
  union all
  select
    'assets_table_size'::text,
    pg_total_relation_size('assets')::numeric,
    'bytes'::text
  union all
  select
    'database_size'::text,
    pg_database_size(current_database())::numeric,
    'bytes'::text;
end;
$$ language plpgsql;

-- Create materialized view for orderbook snapshots (performance optimization)
create materialized view if not exists orderbook_summary as
select
  selling_asset_id,
  buying_asset_id,
  count(*) as offer_count,
  min(price) as min_price,
  max(price) as max_price,
  avg(price) as avg_price,
  sum(amount::numeric) as total_amount,
  max(updated_at) as last_updated
from sdex_offers
group by selling_asset_id, buying_asset_id;

-- Index for orderbook summary
create unique index if not exists idx_orderbook_summary_pair
  on orderbook_summary (selling_asset_id, buying_asset_id);

-- Add function to refresh materialized view
create or replace function refresh_orderbook_summary()
returns void as $$
begin
  refresh materialized view concurrently orderbook_summary;
end;
$$ language plpgsql;

-- Add comments for documentation
comment on table db_health_metrics is 'Stores database health and performance metrics over time';
comment on table archived_offers is 'Archive table for old or inactive offers to keep main table performant';
comment on view active_offers is 'Simplified view of active offers with denormalized asset information';
comment on function archive_old_offers is 'Archives offers older than specified days to maintain performance';
comment on function get_db_health_metrics is 'Returns current database health metrics';
comment on materialized view orderbook_summary is 'Pre-aggregated orderbook statistics for fast queries';
