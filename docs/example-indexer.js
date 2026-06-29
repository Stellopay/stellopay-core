/**
 * Example Stellopay Event Indexer
 * 
 * This script demonstrates how to fetch and parse Stellopay events
 * using the Soroban RPC.
 * 
 * Prerequisites:
 * npm install soroban-client
 */

const { SorobanRpc, xdr, Address } = require('soroban-client');

// Configuration
const RPC_URL = 'https://soroban-testnet.stellar.org';
const CONTRACT_ID = 'CC...'; // Replace with your contract ID
const server = new SorobanRpc.Server(RPC_URL);

/**
 * Parses a Soroban event into a human-readable object
 */
function parseEvent(event) {
  // Topics are XDR encoded
  const topics = event.topic.map(t => xdr.ScVal.fromXDR(t, 'base64'));
  // Event data is XDR encoded
  const data = xdr.ScVal.fromXDR(event.value, 'base64');

  // The first topic is usually the event name (Symbol)
  const eventName = topics[0].sym().toString();

  console.log(`\n[Event] ${eventName} at Ledger ${event.ledger}`);

  // Example: Parsing MilestoneClaimed
  if (eventName === 'MilestoneClaimed') {
    // According to docs/event-indexing.md:
    // agreement_id: u128, milestone_id: u32, amount: i128, to: Address
    // Note: Soroban events usually pack these into a Map or a Struct in the 'data' field
    // depending on how they were published.
    console.log('Parsing MilestoneClaimed details...');
  }

  return {
    name: eventName,
    ledger: event.ledger,
    id: event.id,
  };
}

async function main() {
  console.log('Starting Stellopay Indexer...');

  try {
    // 1. Get the latest ledger to decide where to start
    const latestLedger = await server.getLatestLedger();
    const startLedger = latestLedger.sequence - 100; // Look back 100 ledgers

    console.log(`Scanning from ledger ${startLedger}...`);

    // 2. Fetch events
    const response = await server.getEvents({
      startLedger: startLedger,
      filters: [
        {
          type: 'contract',
          contractIds: [CONTRACT_ID],
        },
      ],
    });

    console.log(`Found ${response.events.length} events.`);

    // 3. Process events
    for (const event of response.events) {
      parseEvent(event);
    }

  } catch (error) {
    console.error('Indexer Error:', error);
  }
}

main();
