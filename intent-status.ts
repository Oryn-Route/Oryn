/**
 * Order Flow Auction (OFA) — intent / round status helpers.
 *
 * UI labels, badge colours, and progress helpers for the intent lifecycle.
 * Pure functions — no React / network dependencies.
 */

import type { IntentStatus, RoundStatus, SolverStatus } from '@/types';

// ── Intent status ─────────────────────────────────────────────────────────────

const INTENT_LABELS: Record<IntentStatus, string> = {
  pending: 'Pending',
  auction_open: 'Auction Open',
  auction_won: 'Auction Won',
  filled: 'Filled',
  abandoned: 'No Solver',
  expired: 'Expired',
  cancelled: 'Cancelled',
};

const INTENT_BADGE_COLORS: Record<IntentStatus, string> = {
  pending: 'bg-neutral-100 text-neutral-700 dark:bg-neutral-800 dark:text-neutral-300',
  auction_open:
    'bg-amber-100 text-amber-800 dark:bg-amber-900/40 dark:text-amber-300',
  auction_won:
    'bg-emerald-100 text-emerald-800 dark:bg-emerald-900/40 dark:text-emerald-300',
  filled:
    'bg-emerald-100 text-emerald-800 dark:bg-emerald-900/40 dark:text-emerald-300',
  abandoned: 'bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-300',
  expired: 'bg-neutral-100 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400',
  cancelled: 'bg-neutral-100 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400',
};

/** Human-readable label for an intent status. */
export function intentStatusLabel(status: IntentStatus): string {
  return INTENT_LABELS[status] ?? status;
}

/** Tailwind class string for a status badge pill. */
export function intentStatusBadge(status: IntentStatus): string {
  return INTENT_BADGE_COLORS[status] ?? INTENT_BADGE_COLORS.pending;
}

/** Whether the intent can still be auctioned. */
export function canOpenAuction(status: IntentStatus): boolean {
  return status === 'pending';
}

/** Whether the intent has reached a terminal state (no further actions). */
export function isTerminalIntent(status: IntentStatus): boolean {
  return status === 'filled' || status === 'expired' || status === 'cancelled';
}

// ── Round status ──────────────────────────────────────────────────────────────

const ROUND_LABELS: Record<RoundStatus, string> = {
  open: 'Open',
  closed: 'Judging',
  settled: 'Settled',
  expired: 'Expired',
};

const ROUND_BADGE_COLORS: Record<RoundStatus, string> = {
  open:
    'bg-sky-100 text-sky-800 dark:bg-sky-900/40 dark:text-sky-300',
  closed:
    'bg-amber-100 text-amber-800 dark:bg-amber-900/40 dark:text-amber-300',
  settled:
    'bg-emerald-100 text-emerald-800 dark:bg-emerald-900/40 dark:text-emerald-300',
  expired: 'bg-neutral-100 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400',
};

/** Human-readable label for a round status. */
export function roundStatusLabel(status: RoundStatus): string {
  return ROUND_LABELS[status] ?? status;
}

/** Tailwind class string for a round status badge pill. */
export function roundStatusBadge(status: RoundStatus): string {
  return ROUND_BADGE_COLORS[status] ?? ROUND_BADGE_COLORS.open;
}

/** Returns 0–100 progress for the auction countdown based on open/close windows. */
export function auctionProgressPercent(
  openedAtMs: number,
  closesAtMs: number,
  nowMs: number = Date.now(),
): number {
  const total = closesAtMs - openedAtMs;
  // Inverted window (closesAt < opensAt) is malformed — no valid progress.
  if (total < 0) return 0;
  // Zero-duration window is instant — always "complete".
  if (total === 0) return 100;
  const elapsed = nowMs - openedAtMs;
  return Math.max(0, Math.min(100, Math.round((elapsed / total) * 100)));
}

// ── Solver status ─────────────────────────────────────────────────────────────

const SOLVER_LABELS: Record<SolverStatus, string> = {
  active: 'Active',
  suspended: 'Suspended',
  slashed: 'Slashed',
};

const SOLVER_BADGE_COLORS: Record<SolverStatus, string> = {
  active:
    'bg-emerald-100 text-emerald-800 dark:bg-emerald-900/40 dark:text-emerald-300',
  suspended:
    'bg-amber-100 text-amber-800 dark:bg-amber-900/40 dark:text-amber-300',
  slashed: 'bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-300',
};

/** Human-readable label for a solver status. */
export function solverStatusLabel(status: SolverStatus): string {
  return SOLVER_LABELS[status] ?? status;
}

/** Tailwind class string for a solver status badge pill. */
export function solverStatusBadge(status: SolverStatus): string {
  return SOLVER_BADGE_COLORS[status] ?? SOLVER_BADGE_COLORS.active;
}

/** Whether the solver is allowed to submit quotes. */
export function canSubmitQuotes(status: SolverStatus): boolean {
  return status === 'active';
}
