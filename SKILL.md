# orange — Lightning Wallet CLI

`orange` is a CLI for the Orange SDK, a graduated-custody Lightning wallet. It gives any AI agent its own Lightning wallet through simple shell commands that output JSON.

Graduated custody means funds start in a trusted Spark backend for instant, low-cost transactions, then automatically move to self-custodial Lightning channels as the balance grows.

## Configuration

Create a `config.toml` (see `config.toml.example`):

```toml
network = "regtest"
storage_path = "/tmp/orange-wallet"
mnemonic = "your twelve word seed phrase here ..."

[chain_source]
type = "esplora"
url = "https://mutinynet.com/api"

[lsp]
address = "185.150.162.100:3551"
node_id = "02a88abd44b3cfc9c0eb7cd93f232dc473de4f66bcea0ee518be70c3b804c90201"

[spark]
sync_interval_secs = 60
prefer_spark_over_lightning = false
```

Pass the config path with `--config`:

```
orange --config /path/to/config.toml <command>
```

Default config path is `config.toml` in the current directory.

## Commands

### balance

Get wallet balance in satoshis.

```
orange balance
```

```json
{
  "trusted_sats": 50000,
  "lightning_sats": 100000,
  "pending_sats": 0,
  "available_sats": 150000
}
```

- `trusted_sats` — balance held in Spark trusted backend
- `lightning_sats` — balance in Lightning channels (self-custodial)
- `pending_sats` — in-flight or unconfirmed balance
- `available_sats` — total spendable (trusted + lightning)

### receive

Generate a single-use BIP21 URI with a BOLT11 invoice for receiving payment.

```
orange receive [--amount <sats>]
```

```json
{
  "invoice": "lnbc500u1p...",
  "address": "bc1q...",
  "amount_sats": 50000,
  "full_uri": "bitcoin:bc1q...?lightning=lnbc500u1p...",
  "from_trusted": false
}
```

- `--amount` — optional amount in satoshis
- `address` — may be `null` if no on-chain address is available
- `from_trusted` — whether this will be received into Spark trusted balance

### receive-offer

Get a reusable BOLT12 offer for receiving payments. Can be shared and paid multiple times.

```
orange receive-offer
```

```json
{
  "offer": "lno1q..."
}
```

### send

Send a payment to a lightning invoice, on-chain address, or BOLT12 offer.

```
orange send <payment> [--amount <sats>]
```

- `payment` — BOLT11 invoice, BOLT12 offer, on-chain address, or BIP21 URI
- `--amount` — required for on-chain addresses and amountless offers

```json
{
  "payment_id": "abcd1234...",
  "amount_sats": 1000,
  "status": "initiated"
}
```

### parse

Parse a payment string and return its details.

```
orange parse <payment>
```

```json
{
  "parsed": "PaymentInstructions { ... }"
}
```

### transactions

List transaction history.

```
orange transactions
```

```json
{
  "count": 2,
  "transactions": [
    {
      "id": "txid123...",
      "status": "Completed",
      "outbound": false,
      "amount_sats": 50000,
      "fee_sats": 100,
      "payment_type": "Lightning",
      "timestamp": 1700000000
    }
  ]
}
```

### channels

List lightning channels.

```
orange channels
```

```json
{
  "count": 1,
  "channels": [
    {
      "channel_id": "ch123...",
      "counterparty_node_id": "02abc...",
      "funding_txo": "txid:0",
      "is_channel_ready": true,
      "is_usable": true,
      "inbound_capacity_sats": 500000,
      "outbound_capacity_sats": 100000,
      "channel_value_sats": 600000
    }
  ]
}
```

### info

Get wallet and node information.

```
orange info
```

```json
{
  "node_id": "02def...",
  "lsp_connected": true,
  "tunables": {
    "trusted_balance_limit_sats": 100000,
    "rebalance_min_sats": 10000,
    "onchain_receive_threshold_sats": 50000,
    "enable_amountless_receive_on_chain": false
  }
}
```

### estimate-fee

Estimate the fee for a payment.

```
orange estimate-fee <payment>
```

```json
{
  "estimated_fee_sats": 150
}
```

## Common Workflows

**Check balance then receive:**
```sh
orange balance
orange receive --amount 50000
# Share the full_uri or invoice with the sender
```

**Send a payment:**
```sh
orange parse "lnbc500u1p..."    # inspect first
orange estimate-fee "lnbc500u1p..."  # check fee
orange send "lnbc500u1p..."     # pay
orange balance                   # verify
```

**Monitor wallet:**
```sh
orange info          # node status and LSP connection
orange channels      # channel health
orange transactions  # payment history
```

## Error Format

All errors are returned as JSON to stdout with a non-zero exit code:

```json
{
  "error": "Failed to get balance: ..."
}
```
