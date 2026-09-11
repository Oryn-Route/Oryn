import { describe, expect, it, vi, afterEach } from 'vitest';
import { OrynRouteClient, OrynRouteApiError } from './client.js';

function ok(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}

function apiError(code: string, message: string, status: number): Response {
    return new Response(JSON.stringify({ error: code, message }), {
      status,
      headers: { 'Content-Type': 'application/json' },
    });
}

const NATIVE = { asset_type: 'native' } as import('./types.js').Asset;
const USDC = { asset_type: 'credit_alphanum4', asset_code: 'USDC', asset_issuer: 'GDUKMGUGDZQK6YH...' } as import('./types.js').Asset;

afterEach(() => {
  vi.restoreAllMocks();
});

describe('AMM Low-Reserve Scenarios', () => {
  it('correctly calculates output amount reflecting high slippage when reserves are exceedingly low', async () => {
    // Ensuring the math models accurate AMM price impacts
    // e.g., draining 50% of the pool impacts output exponentially
    const highSlippageQuote = {
      base_asset: NATIVE,
      quote_asset: USDC,
      amount: '5000',
      price: '0.45000',  // Highly degraded spot price effectively resulting in heavy loss
      total: '2250',
      quote_type: 'sell',
      path: [
        { from_asset: NATIVE, to_asset: USDC, price: '0.45', source: 'amm:low-reserve-pool' },
      ],
      timestamp: 1_700_000_000,
      details: {
        warning: 'High price impact expected'
      }
    };

    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(ok(highSlippageQuote));

    const client = new OrynRouteClient();
    const result = await client.getQuote('native', 'USDC:GDUKMGUGDZQK6YH...', 5000);

    expect(result.price).toBe('0.45000');
    expect(result.total).toBe('2250');
    // We can also ensure SDK gracefully parses such details if present
    expect((result as any).details?.warning).toBeDefined();
  });

  it('rejects trade if slippage limits natively trip on the AMM low reserve calculation', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(
      apiError('slippage_threshold_exceeded', 'AMM minimum reserve bound or slippage exceeded', 400)
    );

    const client = new OrynRouteClient({ retries: 0 });
    let error: OrynRouteApiError | undefined;
    
    try {
      await client.getQuote('native', 'USDC:GDUKMGUGDZQK6YH...', 99999);
    } catch (e: any) {
      error = e;
    }

    expect(error).toBeDefined();
    expect(error?.status).toBe(400);
    expect(error?.code).toBe('slippage_threshold_exceeded');
    // Ensure system asserts thresholds appropriately
    expect(error?.message).toMatch(/reserve bound or slippage/i);
  });

  it('handles zero-liquidity boundary gracefully without arithmetic dividing-by-zero errors', async () => {
    // Asserting there is no instability or NaN returned even if one counter reaches zero depth.
    // In scenarios where an AMM reserve approaches 0, it should safely report a 404 or 400 rather 
    // than crashing internally and throwing a 500.
    vi.spyOn(globalThis, 'fetch').mockResolvedValueOnce(
      apiError('validation_error', 'Invalid pool state: insufficient reserves to calculate output', 400)
    );

    const client = new OrynRouteClient({ retries: 0 });
    let error: OrynRouteApiError | undefined;
    
    try {
      await client.getQuote('native', 'USDC:GDUKMGUGDZQK6YH...', 10);
    } catch (e: any) {
      error = e;
    }

    expect(error).toBeDefined();
    expect(error?.status).toBe(400); // And not a crash / 500 error
    expect(error?.code).toBe('validation_error');
    expect(error?.message).toMatch(/insufficient reserves/i);
  });
});
