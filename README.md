# orange

> [!WARNING]
> Alpha software. This project was largely vibe-coded and likely contains flaws. Do not use it for large sums of money.

A Lightning wallet for AI agents, built on the [Orange SDK](https://github.com/lightningdevkit/orange-sdk). Run the daemon to keep your wallet online and receive real-time payment notifications via webhooks or by polling the event queue.

Orange SDK uses graduated custody — funds start in a trusted Spark backend for instant, low-cost transactions, then automatically move to self-custodial Lightning channels as the balance grows.

## Install

```
cargo install --path .
```

## Quick Start

1. Copy `config.toml.example` to `config.toml` and fill in your settings
2. Start the daemon:

```sh
# Push model: events are POSTed to your endpoints and auto-acknowledged
orange daemon \
  --webhook https://your-app.example.com/payments \
  --webhook https://chat.example.com/notify

# Pull model: events queue up for manual consumption
orange daemon
```

3. Use one-shot commands to interact with the wallet:

```sh
orange receive --amount 50000
orange send "lnbc500u1p..."
orange balance
```

## Setting Up Your Webhook Endpoint

The daemon POSTs a JSON body to each `--webhook` URL whenever a wallet event occurs. Your endpoint should accept `POST` requests with `Content-Type: application/json` and return any 2xx status code. Non-2xx responses and connection errors are logged to stderr but don't block the daemon.

Multiple `--webhook` flags fan out events to different services in parallel (e.g. one for your webstore, one for chat notifications).

When no webhooks are configured, events accumulate in the SDK's persistent queue. Poll them with `get-event` and acknowledge with `event-handled`.

## Configuration

```toml
network = "regtest"
storage_path = "/tmp/orange-wallet"
mnemonic = "your twelve word seed phrase ..."

[chain_source]
type = "esplora"
url = "https://mutinynet.com/api"

[lsp]
address = "185.150.162.100:3551"
node_id = "02a88abd44b3cfc9c0eb7cd93f232dc473de4f66bcea0ee518be70c3b804c90201"
```

See [SKILL.md](SKILL.md) for full command documentation with example JSON output.

## Commands

| Command | Description |
|---|---|
| `daemon` | Run the wallet daemon with optional webhook notifications |
| `get-event` | Get the next pending event from the queue |
| `event-handled` | Acknowledge the current event, advancing the queue |
| `balance` | Get wallet balance |
| `receive` | Generate single-use BIP21 receive URI |
| `receive-offer` | Get reusable BOLT12 offer |
| `send <payment>` | Send a payment |
| `parse <payment>` | Parse a payment string |
| `transactions` | List transaction history |
| `channels` | List lightning channels |
| `info` | Get wallet/node information |
| `estimate-fee <payment>` | Estimate fee for a payment |
| `lightning-address` | Get the wallet's lightning address |
| `register-lightning-address <name>` | Register a lightning address |

## License

[MIT](LICENSE)
