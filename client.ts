/**
 * OrynRoute API client
 *
 * Single source of truth for all frontend-to-backend communication.
 * Covers every REST endpoint exposed by the OrynRoute backend.
 *
 * Base URL defaults to NEXT_PUBLIC_API_URL env var, falling back to
 * http://localhost:8080 (no /api/v1 suffix — paths are added per method).
 */

import type {
  ApiResponse,
  CacheMetricsResponse,
  HealthStatus,
  Orderbook,
  PoolStatsResponse,
  PriceHistoryResponse,
  PairsResponse,
  PriceQuote,
  QuoteType,
  ApiErrorCode,
  RoutesResponse,
  SwapActivityResponse,
  CreateIntentRequest,
  CreateIntentResponse,
  IntentResponse,
  OpenAuctionResponse,
  RegisterSolverRequest,
  SettleAuctionRequest,
  SettleAuctionResponse,
  SolverQuoteResponse,
  SolverResponse,
  SubmitQuoteRequest,
  AuctionStatusResponse,
  OfaSolverAuth,
} from '@/types';

// ---------------------------------------------------------------------------
// Status-page refresh interval — single source of truth matching the design
// spec (auto-refresh every 30 s, matches StatusDashboard.tsx behaviour).
// ---------------------------------------------------------------------------

export const STATUS_PAGE_REFRESH_MS = 30_000;

// ---------------------------------------------------------------------------
// Dependency health response shape (returned by GET /health/deps)
// ---------------------------------------------------------------------------

export interface DepsHealthStatus {
  status: string;
  /** ISO-8601 UTC timestamp */
  timestamp: string;
  components: Record<string, string>;
}

// ---------------------------------------------------------------------------
// Error class
// ---------------------------------------------------------------------------

export class OrynRouteApiError extends Error {
  constructor(
    public readonly status: number,
    public readonly code: ApiErrorCode,
    message: string,
    public readonly details?: unknown,
    public readonly retryAfterMs: number | null = null,
  ) {
    super(message);
    this.name = 'OrynRouteApiError';
  }

  get isRateLimit(): boolean {
    return this.status === 429;
  }

  get isServerError(): boolean {
    return this.status >= 500;
  }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

const DEFAULT_TIMEOUT_MS = 10_000;

/** Sleep for `ms` milliseconds. */
const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

function parseRetryAfterMs(headerValue: string | null): number | null {
  if (!headerValue) {
    return null;
  }

  const seconds = Number(headerValue);
  if (Number.isFinite(seconds) && seconds >= 0) {
    return seconds * 1_000;
  }

  const retryDateMs = Date.parse(headerValue);
  if (Number.isNaN(retryDateMs)) {
    return null;
  }

  return Math.max(0, retryDateMs - Date.now());
}

interface FetchOptions {
  signal?: AbortSignal;
  window?: string;
}

interface ErrorBody {
  error?: ApiErrorCode;
  message?: string;
  details?: unknown;
}

interface BatchQuoteItemResult {
  index: number;
  status: 'ok' | 'error';
  quote?: PriceQuote;
  error?: { code: string; message: string };
}

interface BackendBatchQuoteData {
  results: BatchQuoteItemResult[];
  items_succeeded: number;
  items_failed: number;
  total: number;
  snapshot_timestamp: number;
}

export interface OrynRouteClientOptions {
  baseUrl?: string;
  retries?: number;
}

function parseErrorBody(body: unknown): ErrorBody {
  if (!body || typeof body !== 'object') {
    return {};
  }

  if ('data' in body && body.data && typeof body.data === 'object') {
    const data = body.data as ErrorBody;
    if (data.error) {
      return data;
    }
  }

  const flat = body as ErrorBody;
  if (flat.error) {
    return flat;
  }

  return {};
}

function unwrapEnvelope<T>(body: unknown): T {
  if (body && typeof body === 'object' && 'data' in body) {
    return (body as ApiResponse<T>).data;
  }

  return body as T;
}

function isHealthPayload(value: unknown): value is { status: string } {
  return (
    !!value &&
    typeof value === 'object' &&
    'status' in value &&
    typeof (value as { status: unknown }).status === 'string'
  );
}

function mapBatchQuoteResponse(data: BackendBatchQuoteData): BatchQuoteResponse {
  const quotes: PriceQuote[] = [];

  for (const result of data.results ?? []) {
    if (result.status === 'ok' && result.quote) {
      quotes[result.index] = result.quote;
    }
  }

  return {
    quotes,
    total: data.items_succeeded ?? quotes.filter(Boolean).length,
  };
}

function serializeBatchQuoteRequests(requests: QuoteRequestItem[]) {
  return {
    quotes: requests.map((item) => ({
      base: item.base,
      quote: item.quote,
      ...(item.amount !== undefined ? { amount: String(item.amount) } : {}),
      ...(item.quote_type !== undefined ? { quote_type: item.quote_type } : {}),
    })),
  };
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

export class OrynRouteClient {
  private readonly baseUrl: string;
  private readonly retries: number;

  constructor(baseUrlOrOptions?: string | OrynRouteClientOptions) {
    const proxyEnabled = process.env.NEXT_PUBLIC_API_PROXY === 'true';
    // Static NEXT_PUBLIC_* access only — dynamic process.env[key] is not inlined
    // on Vercel. Prefer shared URL, then testnet staging URL, never leave prod
    // on localhost when only NEXT_PUBLIC_API_URL_TESTNET is configured.
    const defaultBaseUrl = proxyEnabled
      ? ''
      : (process.env.NEXT_PUBLIC_API_URL?.trim() ||
          process.env.NEXT_PUBLIC_API_URL_TESTNET?.trim() ||
          'http://localhost:8080');

    let baseUrl = defaultBaseUrl;
    let retries = 2;

    if (typeof baseUrlOrOptions === 'string') {
      baseUrl = baseUrlOrOptions;
    } else if (baseUrlOrOptions) {
      baseUrl = baseUrlOrOptions.baseUrl ?? defaultBaseUrl;
      if (baseUrlOrOptions.retries !== undefined) {
        retries = baseUrlOrOptions.retries;
      }
    }

    this.baseUrl = baseUrl.replace(/\/$/, '');
    this.retries = retries;
  }

  // -------------------------------------------------------------------------
  // Core fetch wrapper
  // -------------------------------------------------------------------------

  private async request<T>(
    path: string,
    opts: FetchOptions = {},
    retries?: number,
    method: 'GET' | 'POST' = 'GET',
    body?: unknown,
    extraHeaders?: Record<string, string>,
  ): Promise<T> {
    const url = `${this.baseUrl}${path}`;
    const controller = new AbortController();
    const timer = setTimeout(
      () => controller.abort(),
      DEFAULT_TIMEOUT_MS,
    );

    // Honour an external AbortSignal as well
    if (opts.signal?.aborted) {
      controller.abort();
    } else {
      opts.signal?.addEventListener('abort', () => controller.abort());
    }

    const attemptsLeft = retries ?? this.retries;

    try {
      const fetchOptions: RequestInit = {
        method,
        headers: { Accept: 'application/json', ...extraHeaders },
        signal: controller.signal,
      };

      if (body) {
        fetchOptions.body = JSON.stringify(body);
        (fetchOptions.headers as Record<string, string>)['Content-Type'] =
          'application/json';
      }

      const response = await fetch(url, fetchOptions);

      if (!response.ok) {
        const retryAfterMs = parseRetryAfterMs(
          response.headers.get('Retry-After'),
        );

        // Try to parse the backend ErrorResponse body
        let code: ApiErrorCode = 'unknown_error';
        let message = `HTTP ${response.status}`;
        let details: unknown;

        try {
          const errorBody = parseErrorBody(await response.json());
          code = errorBody.error ?? code;
          message = errorBody.message ?? message;
          details = errorBody.details;
        } catch {
          // Body was not JSON — keep defaults
        }

        // Retry on rate-limit (429) and server errors (5xx) with backoff
        if ((response.status === 429 || response.status >= 500) && attemptsLeft > 0) {
          await sleep(retryAfterMs ?? 1_000 * (3 - attemptsLeft));
          return this.request<T>(path, opts, attemptsLeft - 1, method, body, extraHeaders);
        }

        throw new OrynRouteApiError(
          response.status,
          code,
          message,
          details,
          retryAfterMs,
        );
      }

      return response.json() as Promise<T>;
    } catch (err) {
      if (err instanceof OrynRouteApiError) throw err;

      // Network error / timeout
      if (attemptsLeft > 0) {
        await sleep(500 * (3 - attemptsLeft));
        return this.request<T>(path, opts, attemptsLeft - 1, method, body, extraHeaders);
      }

      const message =
        err instanceof Error ? err.message : 'Network error';
      throw new OrynRouteApiError(0, 'network_error' as ApiErrorCode, message);
    } finally {
      clearTimeout(timer);
    }
  }

  // -------------------------------------------------------------------------
  // Public API methods
  // -------------------------------------------------------------------------

  /** GET /health — overall service health check */
  async getHealth(opts?: FetchOptions): Promise<HealthStatus> {
    return this.requestHealth<HealthStatus>('/health', opts);
  }

  /** GET /metrics/cache — quote cache hit/miss metrics */
  getCacheMetrics(opts?: FetchOptions): Promise<CacheMetricsResponse> {
    return this.request<CacheMetricsResponse>('/metrics/cache', opts);
  }

  /** GET /metrics/pool — database connection pool statistics */
  getPoolStats(opts?: FetchOptions): Promise<PoolStatsResponse> {
    return this.request<PoolStatsResponse>('/metrics/pool', opts);
  }

  /** GET /health/deps — external dependency health check */
  async getDepsHealth(opts?: FetchOptions): Promise<DepsHealthStatus> {
    return this.requestHealth<DepsHealthStatus>('/health/deps', opts);
  }

  /**
   * Health endpoints return an ApiResponse envelope and may use HTTP 503 when
   * degraded/unhealthy while still including a useful status payload.
   */
  private async requestHealth<T extends { status: string }>(
    path: string,
    opts: FetchOptions = {},
  ): Promise<T> {
    const url = `${this.baseUrl}${path}`;
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), DEFAULT_TIMEOUT_MS);

    if (opts.signal?.aborted) {
      controller.abort();
    } else {
      opts.signal?.addEventListener('abort', () => controller.abort());
    }

    try {
      const response = await fetch(url, {
        method: 'GET',
        headers: { Accept: 'application/json' },
        signal: controller.signal,
      });

      let body: unknown;
      try {
        body = await response.json();
      } catch {
        body = null;
      }

      const payload = unwrapEnvelope<unknown>(body);
      if (isHealthPayload(payload)) {
        return payload as T;
      }

      if (!response.ok) {
        const errorBody = parseErrorBody(body);
        throw new OrynRouteApiError(
          response.status,
          errorBody.error ?? 'unknown_error',
          errorBody.message ?? `HTTP ${response.status}`,
          errorBody.details,
        );
      }

      throw new OrynRouteApiError(
        response.status,
        'unknown_error',
        'Invalid health response payload',
      );
    } catch (err) {
      if (err instanceof OrynRouteApiError) throw err;
      const message = err instanceof Error ? err.message : 'Network error';
      throw new OrynRouteApiError(0, 'network_error' as ApiErrorCode, message);
    } finally {
      clearTimeout(timer);
    }
  }

  /** GET /api/v1/pairs — list all trading pairs */
  getPairs(opts?: FetchOptions): Promise<PairsResponse> {
    return this.request<PairsResponse>('/api/v1/pairs', opts);
  }

  /** GET /api/v1/activity/swaps — list recent swap activity */
  async getSwapActivity(
    params?: { limit?: number; before_ledger?: number },
    opts?: FetchOptions,
  ): Promise<SwapActivityResponse> {
    const query = new URLSearchParams();
    if (params?.limit !== undefined) query.set('limit', String(params.limit));
    if (params?.before_ledger !== undefined) query.set('before_ledger', String(params.before_ledger));
    const qs = query.toString();
    const path = `/api/v1/activity/swaps${qs ? `?${qs}` : ''}`;
    const body = await this.request<ApiResponse<SwapActivityResponse> | SwapActivityResponse>(
      path,
      opts,
    );
    return unwrapEnvelope<SwapActivityResponse>(body);
  }

  /**
   * GET /api/v1/orderbook/{base}/{quote}
   *
   * @param base  Asset identifier: "native" | "CODE" | "CODE:ISSUER"
   * @param quote Asset identifier: "native" | "CODE" | "CODE:ISSUER"
   */
  getOrderbook(
    base: string,
    quote: string,
    opts?: FetchOptions,
  ): Promise<Orderbook> {
    const path = `/api/v1/orderbook/${encodeURIComponent(base)}/${encodeURIComponent(quote)}`;
    return this.request<Orderbook>(path, opts);
  }

  /**
   * GET /api/v1/routes/{base}/{quote} — ranked route candidates
   */
  async getRoutes(
    base: string,
    quote: string,
    amount?: number,
    limit?: number,
    maxHops?: number,
    opts?: FetchOptions,
  ): Promise<RoutesResponse> {
    const params = new URLSearchParams();
    if (amount !== undefined) params.set('amount', String(amount));
    if (limit !== undefined) params.set('limit', String(limit));
    if (maxHops !== undefined) params.set('max_hops', String(maxHops));
    const qs = params.toString();
    const path = `/api/v1/routes/${encodeURIComponent(base)}/${encodeURIComponent(quote)}${qs ? `?${qs}` : ''}`;
    const body = await this.request<ApiResponse<RoutesResponse> | RoutesResponse>(
      path,
      opts,
    );
    return unwrapEnvelope<RoutesResponse>(body);
  }

  /**
   * GET /api/v1/quote/{base}/{quote}?amount={amount}&quote_type={sell|buy}
   *
   * Unwraps the API envelope and captures the server `request_id` from the
   * response body and `x-request-id` header for diagnostics correlation.
   */
  async getQuote(
    base: string,
    quote: string,
    amount?: number,
    type: QuoteType = 'sell',
    opts?: FetchOptions,
  ): Promise<QuoteFetchResult> {
    const params = new URLSearchParams({ quote_type: type });
    if (amount !== undefined) params.set('amount', String(amount));
    const path = `/api/v1/quote/${encodeURIComponent(base)}/${encodeURIComponent(quote)}?${params}`;
    return this.requestQuote(path, opts);
  }

  private async requestQuote(
    path: string,
    opts: FetchOptions = {},
    retries?: number,
  ): Promise<QuoteFetchResult> {
    const url = `${this.baseUrl}${path}`;
    const controller = new AbortController();
    const timer = setTimeout(
      () => controller.abort(),
      DEFAULT_TIMEOUT_MS,
    );

    opts.signal?.addEventListener('abort', () => controller.abort(), { once: true });
    if (opts.signal?.aborted) {
      controller.abort();
    }

    const attemptsLeft = retries ?? this.retries;

    try {
      const response = await fetch(url, {
        method: 'GET',
        headers: { Accept: 'application/json' },
        signal: controller.signal,
      });

      if (!response.ok) {
        const retryAfterMs = parseRetryAfterMs(
          response.headers.get('Retry-After'),
        );

        let code: ApiErrorCode = 'unknown_error';
        let message = `HTTP ${response.status}`;
        let details: unknown;

        try {
          const errorBody = parseErrorBody(await response.json());
          code = errorBody.error ?? code;
          message = errorBody.message ?? message;
          details = errorBody.details;
        } catch {
          // Body was not JSON — keep defaults
        }

        if ((response.status === 429 || response.status >= 500) && attemptsLeft > 0) {
          await sleep(retryAfterMs ?? 1_000 * (3 - attemptsLeft));
          return this.requestQuote(path, opts, attemptsLeft - 1);
        }

        throw new OrynRouteApiError(
          response.status,
          code,
          message,
          details,
          retryAfterMs,
        );
      }

      const headerRequestId = response.headers.get('x-request-id');
      const body = (await response.json()) as
        | ApiResponse<PriceQuote>
        | PriceQuote;

      if (body && typeof body === 'object' && 'data' in body) {
        const envelope = body as ApiResponse<PriceQuote>;
        return {
          quote: envelope.data,
          requestId: envelope.request_id || headerRequestId || generateFallbackRequestId(),
        };
      }

      return {
        quote: body as PriceQuote,
        requestId: headerRequestId || generateFallbackRequestId(),
      };
    } catch (err) {
      if (err instanceof OrynRouteApiError) throw err;

      if (attemptsLeft > 0) {
        await sleep(500 * (3 - attemptsLeft));
        return this.requestQuote(path, opts, attemptsLeft - 1);
      }

      const message =
        err instanceof Error ? err.message : 'Network error';
      throw new OrynRouteApiError(0, 'network_error' as ApiErrorCode, message);
    } finally {
      clearTimeout(timer);
    }
  }

  /**
   * GET /api/v1/price-history/{base}/{quote}
   */
  getPriceHistory(
    base: string,
    quote: string,
    opts?: FetchOptions,
  ): Promise<PriceHistoryResponse> {
    const params = new URLSearchParams();
    if (opts?.window) params.set('window', opts.window);
    const qs = params.toString();
    const path = `/api/v1/price-history/${encodeURIComponent(base)}/${encodeURIComponent(quote)}${qs ? `?${qs}` : ''}`;
    return this.request<PriceHistoryResponse>(path, opts);
  }

  /**
   * POST /api/v1/batch/quote — fetch multiple price quotes in a single request.
   *
   * @param requests Array of quote requests to fetch.
   *
   * @throws {OrynRouteApiError} when the batch request fails.
   */
  async getQuotesBatch(
    requests: QuoteRequestItem[],
    opts?: FetchOptions,
  ): Promise<BatchQuoteResponse> {
    const path = '/api/v1/batch/quote';
    const body = await this.request<
      ApiResponse<BackendBatchQuoteData> | BackendBatchQuoteData
    >(path, opts, undefined, 'POST', serializeBatchQuoteRequests(requests));
    return mapBatchQuoteResponse(unwrapEnvelope<BackendBatchQuoteData>(body));
  }

  /**
   * POST /api/v1/swap/prepare — build an unsigned swap envelope + quote id.
   */
  async prepareSwap(
    params: SwapPrepareRequest,
    opts?: FetchOptions,
  ): Promise<PreparedSwapResponse> {
    const body = await this.request<
      ApiResponse<PreparedSwapResponse> | PreparedSwapResponse
    >('/api/v1/swap/prepare', opts, undefined, 'POST', params);
    return unwrapEnvelope<PreparedSwapResponse>(body);
  }

  /**
   * POST /api/v1/swap/submit — broadcast a wallet-signed envelope.
   *
   * Default `retries` is 0 so callers (API execution) own ambiguous-submit
   * retry policy with the exact same signed body.
   */
  async submitSwap(
    params: SwapSubmitRequest,
    opts?: FetchOptions & { retries?: number },
  ): Promise<SwapSubmitResponse> {
    const body = await this.request<
      ApiResponse<SwapSubmitResponse> | SwapSubmitResponse
    >('/api/v1/swap/submit', opts, opts?.retries ?? 0, 'POST', params);
    return unwrapEnvelope<SwapSubmitResponse>(body);
  }

  // -------------------------------------------------------------------------
  // Order Flow Auction (OFA) — /api/v2
  // -------------------------------------------------------------------------

  /** POST /api/v2/intents — submit a signed-but-unbroadcast intent. */
  async createIntent(
    params: CreateIntentRequest,
    opts?: FetchOptions,
  ): Promise<CreateIntentResponse> {
    const body = await this.request<
      ApiResponse<CreateIntentResponse> | CreateIntentResponse
    >('/api/v2/intents', opts, undefined, 'POST', params);
    return unwrapEnvelope<CreateIntentResponse>(body);
  }

  /** GET /api/v2/intents/{intentId} — intent status. */
  async getIntent(
    intentId: string,
    opts?: FetchOptions,
  ): Promise<IntentResponse> {
    const body = await this.request<
      ApiResponse<IntentResponse> | IntentResponse
    >(`/api/v2/intents/${encodeURIComponent(intentId)}`, opts);
    return unwrapEnvelope<IntentResponse>(body);
  }

  /** POST /api/v2/auctions/{intentId}/open — open the auction window. */
  async openAuction(
    intentId: string,
    opts?: FetchOptions,
  ): Promise<OpenAuctionResponse> {
    const body = await this.request<
      ApiResponse<OpenAuctionResponse> | OpenAuctionResponse
    >(`/api/v2/auctions/${encodeURIComponent(intentId)}/open`, opts, undefined, 'POST', {});
    return unwrapEnvelope<OpenAuctionResponse>(body);
  }

  /** POST /api/v2/solvers/register — register a solver (bond mirrored on-chain). */
  async registerSolver(
    params: RegisterSolverRequest,
    opts?: FetchOptions,
  ): Promise<SolverResponse> {
    const body = await this.request<
      ApiResponse<SolverResponse> | SolverResponse
    >('/api/v2/solvers/register', opts, undefined, 'POST', params);
    return unwrapEnvelope<SolverResponse>(body);
  }

  /** GET /api/v2/solvers/{solverId} — solver record. */
  async getSolver(
    solverId: string,
    opts?: FetchOptions,
  ): Promise<SolverResponse> {
    const body = await this.request<
      ApiResponse<SolverResponse> | SolverResponse
    >(`/api/v2/solvers/${encodeURIComponent(solverId)}`, opts);
    return unwrapEnvelope<SolverResponse>(body);
  }

  /** POST /api/v2/auctions/{roundId}/quotes — solver bid (solver-authed). */
  async submitQuote(
    roundId: string,
    params: SubmitQuoteRequest,
    auth: OfaSolverAuth,
    opts?: FetchOptions,
  ): Promise<SolverQuoteResponse> {
    const body = await this.request<
      ApiResponse<SolverQuoteResponse> | SolverQuoteResponse
    >(
      `/api/v2/auctions/${encodeURIComponent(roundId)}/quotes`,
      opts,
      undefined,
      'POST',
      params,
      {
        'x-solver-id': auth.solverId,
        'x-solver-key': auth.solverKey,
      },
    );
    return unwrapEnvelope<SolverQuoteResponse>(body);
  }

  /** GET /api/v2/auctions/{roundId}/quotes — winner-visible auction status. */
  async getAuctionStatus(
    roundId: string,
    opts?: FetchOptions,
  ): Promise<AuctionStatusResponse> {
    const body = await this.request<
      ApiResponse<AuctionStatusResponse> | AuctionStatusResponse
    >(`/api/v2/auctions/${encodeURIComponent(roundId)}/quotes`, opts);
    return unwrapEnvelope<AuctionStatusResponse>(body);
  }

  /** POST /api/v2/auctions/{roundId}/settle — winner settlement report. */
  async settleAuction(
    roundId: string,
    params: SettleAuctionRequest,
    auth: OfaSolverAuth,
    opts?: FetchOptions,
  ): Promise<SettleAuctionResponse> {
    const body = await this.request<
      ApiResponse<SettleAuctionResponse> | SettleAuctionResponse
    >(
      `/api/v2/auctions/${encodeURIComponent(roundId)}/settle`,
      opts,
      undefined,
      'POST',
      params,
      {
        'x-solver-id': auth.solverId,
        'x-solver-key': auth.solverKey,
      },
    );
    return unwrapEnvelope<SettleAuctionResponse>(body);
  }
}

/** Hop shape accepted by prepare/simulate route bodies. */
export interface SwapRouteHop {
  from_asset: string;
  to_asset: string;
  source: string;
  fee_bps?: number;
  price?: string;
  venue_ref?: string;
}

/** Request body for POST /api/v1/swap/prepare. */
export interface SwapPrepareRequest {
  route: { hops: SwapRouteHop[] };
  amount: string;
  sender: string;
  min_output?: string;
  slippage_bps?: number;
}

/** Inner data from POST /api/v1/swap/prepare. */
export interface PreparedSwapResponse {
  quote_id: string;
  xdr_envelope: string;
  expected_output: string;
  min_output?: string;
  expires_at: number;
  /** Always `classic_path_payment` on success. */
  execution_mode: 'classic_path_payment' | string;
  /** Network passphrase the unsigned envelope was built for (compare before wallet signing). */
  network_passphrase: string;
}

/** Request body for POST /api/v1/swap/submit. */
export interface SwapSubmitRequest {
  quote_id: string;
  signed_xdr: string;
}

/** Inner data from POST /api/v1/swap/submit. */
export interface SwapSubmitResponse {
  quote_id: string;
  tx_hash: string;
  status: string;
  output_amount?: string;
  ledger?: number;
}

/** Single request item for a batch quote. */
export interface QuoteRequestItem {
  base: string;
  quote: string;
  amount?: number;
  quote_type?: QuoteType;
}

/** Response from a batch quote request. */
export interface BatchQuoteResponse {
  quotes: PriceQuote[];
  total: number;
}

/** Result of a single quote fetch including server correlation metadata. */
export interface QuoteFetchResult {
  quote: PriceQuote;
  requestId: string;
}

function generateFallbackRequestId(): string {
  return `req_${Date.now()}_${Math.random().toString(36).substring(2, 9)}`;
}

// ---------------------------------------------------------------------------
// Factory + singleton — use factory in network-aware hooks
// ---------------------------------------------------------------------------

export function createOrynRouteClient(baseUrl?: string): OrynRouteClient {
  return new OrynRouteClient(baseUrl);
}

export const orynRouteClient = createOrynRouteClient();
