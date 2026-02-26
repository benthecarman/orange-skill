# Agent Payment Flows

This guide covers how to build payment-accepting applications (like a webstore) on top of `orange`.

## Webstore Flow

The basic pattern: generate a unique invoice per order, store the payment hash, and match it when the webhook fires.

### Setup

1. Start the daemon with a webhook pointing at your webstore backend:

```sh
orange daemon --webhook https://your-store.example.com/api/payments
```

2. Optionally register a lightning address for your store:

```sh
orange register-lightning-address "mystore"
# => mystore@breez.tips
```

### Creating an Order

When a customer checks out, generate an invoice for the order amount:

```sh
orange receive --amount 5000
```

```json
{
  "invoice": "lnbc50u1p...",
  "address": "bc1q...",
  "amount_sats": 5000,
  "full_uri": "bitcoin:bc1q...?lightning=lnbc50u1p...",
  "from_trusted": false
}
```

Store the mapping in your database:

```
order_id: "order-123"
invoice: "lnbc50u1p..."
amount_sats: 5000
status: "pending"
```

Display the `invoice` or `full_uri` to the customer as a QR code or payment link.

### Handling Payment Confirmation

When the customer pays, the daemon POSTs to your webhook:

```json
{
  "type": "payment_received",
  "timestamp": 1700000000,
  "payment_id": "SC-abcd1234...",
  "payment_hash": "e3b0c44298fc1c14...",
  "amount_msat": 5000000,
  "amount_sats": 5000,
  "custom_records_count": 0,
  "lsp_fee_msats": null
}
```

Your webhook handler should:

1. Look up the order by `payment_id` or `payment_hash`
2. Verify `amount_sats` matches the expected amount
3. Mark the order as paid
4. Fulfill the order (send product, unlock content, etc.)

### Example Webhook Handler (pseudocode)

```python
def handle_webhook(request):
    event = request.json

    if event["type"] != "payment_received":
        return 200  # ignore non-payment events

    order = db.find_order_by_payment_hash(event["payment_hash"])
    if not order:
        log.warn(f"Unknown payment: {event['payment_hash']}")
        return 200

    if order.status == "paid":
        return 200  # idempotent

    if event["amount_sats"] < order.amount_sats:
        log.warn(f"Underpaid order {order.id}")
        return 200

    order.status = "paid"
    order.paid_at = event["timestamp"]
    db.save(order)

    fulfill_order(order)
    return 200
```

### Matching Payments to Orders

There are two ways to correlate incoming payments with orders:

**By payment_hash** — When you call `orange receive`, the returned invoice encodes a payment hash. Parse the BOLT11 invoice to extract it and store it alongside the order. When the webhook fires, match on `event["payment_hash"]`.

**By payment_id** — The `payment_id` in the event is the SDK's internal identifier. You can also store this if you extract it from transaction history after generating the invoice.

### Checking Payment Status

If you need to verify a payment outside the webhook flow (e.g. customer claims they paid):

```sh
orange transactions
```

Search the returned transactions list for a matching payment.

## Pull Model (No Webhook)

If you prefer polling over webhooks, run the daemon without `--webhook` and poll from your application:

```sh
# Start daemon
orange daemon

# Poll loop (in your application)
while true:
    event = shell("orange get-event")
    if event["event"] is null:
        sleep(1)
        continue
    process(event)
    shell("orange event-handled")
```

This is simpler to set up but adds latency equal to your poll interval.
