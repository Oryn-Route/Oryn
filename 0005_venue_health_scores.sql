-- Migration: 0005_venue_health_scores
-- Adds the venue_health_scores table for persisting computed health scores
-- and their contributing signal values per liquidity source.

create table if not exists venue_health_scores (
    id            bigserial primary key,
    venue_ref     text          not null,
    venue_type    text          not null check (venue_type in ('sdex', 'amm')),
    score         numeric(5, 4) not null check (score >= 0 and score <= 1),
    signals       jsonb         not null default '{}',
    computed_at   timestamptz   not null default now()
);

create index if not exists idx_venue_health_scores_ref_time
    on venue_health_scores (venue_ref, computed_at desc);
