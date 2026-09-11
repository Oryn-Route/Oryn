import { OrynRouteClient } from '../src/index.js';

const client = new OrynRouteClient('http://localhost:8080');

async function main(): Promise<void> {
  const routes = await client.getRoutes(
    'native',
    'USDC:GDUKMGUGDZQK6YH...',
    100,
    'sell',
  );

  console.log('Route steps');
  console.log('-----------');
  routes.forEach((step, index) => {
    console.log(
      `${index + 1}. ${step.from_asset.asset_code ?? 'XLM'} -> ${step.to_asset.asset_code ?? 'XLM'} via ${step.source} @ ${step.price}`,
    );
  });
}

main().catch((error) => {
  console.error('Quickstart routes example failed:', error);
  process.exitCode = 1;
});
