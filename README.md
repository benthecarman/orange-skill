# orange

A CLI for the [Orange SDK](https://github.com/lightningdevkit/orange-sdk) Lightning wallet. Every command outputs JSON to stdout, making it easy for AI agents and scripts to operate a Lightning wallet.

Orange SDK is a graduated-custody Lightning wallet — funds start in a trusted Spark backend for instant, low-cost transactions, then automatically move to self-custodial Lightning channels as the balance grows.

## Install

```
cargo install --path .
```

## Usage

```
orange --config config.toml <command>
```

Commands:

| Command | Description |
|---|---|
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

## Configuration

Copy `config.toml.example` to `config.toml` and fill in your settings:

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

## License

[MIT](LICENSE)
