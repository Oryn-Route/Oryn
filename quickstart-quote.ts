import { OrynRouteClient } from '../src/index.js';

const client = new OrynRouteClient('http://localhost:8080');

async function main(): Promise<void> {
  const quote = await client.getQuote('native', 'USDC:GDUKMGUGDZQK6YH...', 100);

  console.log('Price quote');
  console.log('-----------');
  console.log(`Input amount: ${quote.amount}`);
  console.log(`Price: ${quote.price}`);
  console.log(`Output total: ${quote.total}`);
  console.log(`Hops: ${quote.path.length}`);
}

main().catch((error) => {
  console.error('Quickstart quote example failed:', error);
  process.exitCode = 1;
});
