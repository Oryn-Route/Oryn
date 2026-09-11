import type { Asset, PathStep } from '@/types';

export function getAssetCode(asset: Asset): string {
  if (asset.asset_type === 'native') return 'XLM';
  return asset.asset_code || 'UNKNOWN';
}

export function parseSource(source: string): {
  isSDEX: boolean;
  poolName?: string;
} {
  if (source === 'sdex') {
    return { isSDEX: true };
  }
  if (source.startsWith('amm:')) {
    const poolAddress = source.substring(4);
    return {
      isSDEX: false,
      poolName: `Pool ${poolAddress.substring(0, 8)}...`,
    };
  }
  return { isSDEX: false, poolName: source };
}

/** Plain-language description for screen readers and aria-labels */
export function describeTradeRoute(path: PathStep[]): string {
  if (!path.length) return 'No route available.';

  const hopDescriptions = path.map((step, i) => {
    const from = getAssetCode(step.from_asset);
    const to = getAssetCode(step.to_asset);
    const { isSDEX, poolName } = parseSource(step.source);
    const venue = isSDEX
      ? 'Stellar DEX order book'
      : poolName || 'AMM liquidity pool';
    return `Hop ${i + 1}: ${from} to ${to} through ${venue}`;
  });

  const start = getAssetCode(path[0].from_asset);
  const end = getAssetCode(path[path.length - 1].to_asset);

  return `Route from ${start} to ${end}, ${path.length} ${path.length === 1 ? 'hop' : 'hops'}. ${hopDescriptions.join('. ')}.`;
}
