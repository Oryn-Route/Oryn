'use client';

/**
 * Orchestrates the user-facing OFA intent lifecycle:
 *
 *   createIntent → openAuction → poll auction status → settled / expired
 *
 * Callers supply an `intentId` once `createIntent` resolves; the hook polls
 * `getAuctionStatus(roundId)` every `pollMs` until the round closes or expires.
 *
 * For solver-facing workflows (submitQuote / settleAuction) call the client
 * methods directly — this hook targets the intent-submitter role only.
 */

import { useCallback, useEffect, useRef, useState } from 'react';

import { OrynRouteApiError } from '@/lib/api/client';
import { useOrynRouteClient } from '@/hooks/useOrynRouteClient';
import type {
  AuctionStatusResponse,
  CreateIntentRequest,
  CreateIntentResponse,
  IntentResponse,
  OpenAuctionResponse,
  RoundStatus,
} from '@/types';

// ── Public state shape ────────────────────────────────────────────────────────

export interface UseIntentAuctionState {
  /** The intent once createIntent succeeds. Undefined until created. */
  intent: IntentResponse | undefined;
  /** The auction round once openAuction succeeds. Undefined until opened. */
  round: OpenAuctionResponse | undefined;
  /** Polled auction status while the round is open / being judged. */
  auctionStatus: AuctionStatusResponse | undefined;
  /** True while any in-flight request is executing. */
  loading: boolean;
  /** Last error, if any. */
  error: OrynRouteApiError | Error | null;
  /** Current round status — convenient for switch/case branching. */
  roundStatus: RoundStatus | undefined;
}

export interface UseIntentAuctionActions {
  /** Submit a new intent. Sets `intent` on success. */
  createIntent: (request: CreateIntentRequest) => Promise<CreateIntentResponse>;
  /** Open the auction for the previously-created intent. Sets `round`. */
  openAuction: (intentId: string) => Promise<OpenAuctionResponse>;
  /** Reset all state (e.g. to start a fresh flow). */
  reset: () => void;
}

export interface UseIntentAuctionOptions {
  /** Polling interval for auction status while round is open/closed (default 1 500 ms). */
  pollMs?: number;
  /** AbortSignal forwarded to all fetch calls (e.g. parent component unmount). */
  signal?: AbortSignal;
}

const INITIAL_STATE: UseIntentAuctionState = {
  intent: undefined,
  round: undefined,
  auctionStatus: undefined,
  loading: false,
  error: null,
  roundStatus: undefined,
};

export interface UseIntentAuctionResult extends UseIntentAuctionState, UseIntentAuctionActions {}

const POLLABLE = new Set<RoundStatus>(['open', 'closed']);

export function useIntentAuction({
  pollMs = 1_500,
  signal: externalSignal,
}: UseIntentAuctionOptions = {}): UseIntentAuctionResult {
  const client = useOrynRouteClient();
  const [state, setState] = useState<UseIntentAuctionState>(INITIAL_STATE);
  const pollTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // ── Actions ──────────────────────────────────────────────────────────────

  const createIntent = useCallback(
    async (request: CreateIntentRequest): Promise<CreateIntentResponse> => {
      setState((s) => ({ ...s, loading: true, error: null }));
      try {
        const res = await client.createIntent(request, { signal: externalSignal });
        const intentBody = await client.getIntent(res.intent_id, { signal: externalSignal });
        setState((s) => ({
          ...s,
          intent: intentBody,
          loading: false,
        }));
        return res;
      } catch (err) {
        const error =
          err instanceof Error ? err : new Error(String(err));
        setState((s) => ({ ...s, loading: false, error }));
        throw err;
      }
    },
    [client, externalSignal],
  );

  const openAuction = useCallback(
    async (intentId: string): Promise<OpenAuctionResponse> => {
      setState((s) => ({ ...s, loading: true, error: null }));
      try {
        const res = await client.openAuction(intentId, { signal: externalSignal });
        setState((s) => ({
          ...s,
          round: res,
          roundStatus: res.status,
          loading: false,
        }));
        return res;
      } catch (err) {
        const error =
          err instanceof Error ? err : new Error(String(err));
        setState((s) => ({ ...s, loading: false, error }));
        throw err;
      }
    },
    [client, externalSignal],
  );

  const reset = useCallback(() => {
    if (pollTimerRef.current) {
      clearInterval(pollTimerRef.current);
      pollTimerRef.current = null;
    }
    setState(INITIAL_STATE);
  }, []);

  // ── Auction status polling ───────────────────────────────────────────────

  useEffect(() => {
    if (!state.round) return;

    const roundId = state.round.round_id;
    let cancelled = false;

    const poll = async () => {
      if (cancelled) return;
      try {
        const status = await client.getAuctionStatus(roundId, { signal: externalSignal });
        if (cancelled) return;
        setState((s) => ({
          ...s,
          auctionStatus: status,
          roundStatus: status.status,
        }));

        if (!POLLABLE.has(status.status)) {
          // Terminal — stop polling.
          if (pollTimerRef.current) {
            clearInterval(pollTimerRef.current);
            pollTimerRef.current = null;
          }
        }
      } catch {
        // Transient poll failure — leave timer running; next tick will retry.
      }
    };

    // Initial fetch + interval
    poll();
    pollTimerRef.current = setInterval(poll, pollMs);

    return () => {
      cancelled = true;
      if (pollTimerRef.current) {
        clearInterval(pollTimerRef.current);
        pollTimerRef.current = null;
      }
    };
  }, [state.round?.round_id, client, externalSignal, pollMs]);

  return { ...state, createIntent, openAuction, reset };
}
