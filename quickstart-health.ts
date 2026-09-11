import { OrynRouteClient } from '../src/index.js';

const client = new OrynRouteClient('http://localhost:8080');

async function main(): Promise<void> {
  const health = await client.getHealth();

  console.log('OrynRoute health');
  console.log('-------------------');
  console.log(`Status: ${health.status}`);
  console.log(`Version: ${health.version}`);
  console.log(`Checked at: ${health.timestamp}`);
  for (const [component, status] of Object.entries(health.components)) {
    console.log(`${component}: ${status}`);
  }
}

main().catch((error) => {
  console.error('Quickstart health example failed:', error);
  process.exitCode = 1;
});
