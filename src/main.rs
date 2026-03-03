mod config;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::{Parser, Subcommand};
use config::Config;
use orange_sdk::bitcoin::hex::DisplayHex;
use orange_sdk::bitcoin_payment_instructions::amount::Amount;
use orange_sdk::{Event, PaymentInfo, Wallet};
use serde_json::json;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "orange", about = "Orange SDK Lightning wallet CLI")]
struct Cli {
    /// Path to config.toml
    #[arg(long, default_value = "config.toml")]
    config: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Get wallet balance
    Balance,
    /// Generate single-use BIP21 receive URI
    Receive {
        /// Amount in satoshis (optional)
        #[arg(long)]
        amount: Option<u64>,
    },
    /// Get reusable BOLT12 offer
    ReceiveOffer,
    /// Send a payment
    Send {
        /// Lightning invoice, on-chain address, BOLT12 offer, or BIP21 URI
        payment: String,
        /// Amount in satoshis (required for addresses and amountless offers)
        #[arg(long)]
        amount: Option<u64>,
    },
    /// Parse a payment string
    Parse {
        /// Payment string to parse
        payment: String,
    },
    /// List transaction history
    Transactions,
    /// List lightning channels
    Channels,
    /// Get wallet/node information
    Info,
    /// Estimate fee for a payment
    EstimateFee {
        /// Payment string to estimate fee for
        payment: String,
    },
    /// Get the wallet's lightning address
    LightningAddress,
    /// Register a lightning address for this wallet
    RegisterLightningAddress {
        /// Username for the lightning address (e.g. "alice" for alice@breez.tips)
        name: String,
    },
    /// Run as a long-lived daemon, listening for wallet events
    Daemon {
        /// Webhook URL, optionally with a Bearer token: "url" or "url|token"
        #[arg(long)]
        webhook: Vec<String>,
    },
    /// Get the next pending event from the wallet event queue
    GetEvent,
    /// Mark the current event as handled, removing it from the queue
    EventHandled,
}

// --- Request types for daemon HTTP API ---

#[derive(serde::Deserialize)]
struct ReceiveRequest {
    amount: Option<u64>,
}

#[derive(serde::Deserialize)]
struct SendRequest {
    payment: String,
    amount: Option<u64>,
}

#[derive(serde::Deserialize)]
struct PaymentRequest {
    payment: String,
}

#[derive(serde::Deserialize)]
struct RegisterAddressRequest {
    name: String,
}

// --- Main ---

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let config = match Config::load(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            print_error(&e);
            std::process::exit(1);
        }
    };

    let daemon_host = config.daemon.host.clone();
    let daemon_port = config.daemon.port;

    if let Command::Daemon { webhook } = cli.command {
        // Server mode: initialize wallet, start HTTP server + event loop
        let wallet_config = match config.into_wallet_config() {
            Ok(c) => c,
            Err(e) => {
                print_error(&e);
                std::process::exit(1);
            }
        };

        let wallet = match Wallet::new(wallet_config).await {
            Ok(w) => w,
            Err(e) => {
                print_error(&format!("Failed to initialize wallet: {e:?}"));
                std::process::exit(1);
            }
        };

        let wallet = Arc::new(wallet);
        run_daemon(wallet, &daemon_host, daemon_port, &webhook).await;
    } else {
        // Client mode: call daemon via HTTP
        let base_url = format!("http://{daemon_host}:{daemon_port}");
        let result = rpc_call(&base_url, cli.command).await;
        match result {
            Ok(value) => {
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
                if value.get("error").is_some() {
                    std::process::exit(1);
                }
            }
            Err(e) => {
                print_error(&e);
                std::process::exit(1);
            }
        }
    }
}

fn print_error(msg: &str) {
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"error": msg})).unwrap()
    );
}

// --- RPC Client (non-daemon commands call the daemon over HTTP) ---

async fn rpc_call(base_url: &str, command: Command) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();

    let response = match command {
        Command::Balance => client.get(format!("{base_url}/balance")).send().await,
        Command::Receive { amount } => {
            client
                .post(format!("{base_url}/receive"))
                .json(&json!({ "amount": amount }))
                .send()
                .await
        }
        Command::ReceiveOffer => client.get(format!("{base_url}/receive-offer")).send().await,
        Command::Send { payment, amount } => {
            client
                .post(format!("{base_url}/send"))
                .json(&json!({ "payment": payment, "amount": amount }))
                .send()
                .await
        }
        Command::Parse { payment } => {
            client
                .post(format!("{base_url}/parse"))
                .json(&json!({ "payment": payment }))
                .send()
                .await
        }
        Command::Transactions => client.get(format!("{base_url}/transactions")).send().await,
        Command::Channels => client.get(format!("{base_url}/channels")).send().await,
        Command::Info => client.get(format!("{base_url}/info")).send().await,
        Command::EstimateFee { payment } => {
            client
                .post(format!("{base_url}/estimate-fee"))
                .json(&json!({ "payment": payment }))
                .send()
                .await
        }
        Command::LightningAddress => {
            client
                .get(format!("{base_url}/lightning-address"))
                .send()
                .await
        }
        Command::RegisterLightningAddress { name } => {
            client
                .post(format!("{base_url}/register-lightning-address"))
                .json(&json!({ "name": name }))
                .send()
                .await
        }
        Command::GetEvent => client.get(format!("{base_url}/get-event")).send().await,
        Command::EventHandled => client.post(format!("{base_url}/event-handled")).send().await,
        Command::Daemon { .. } => unreachable!(),
    }
    .map_err(|e| format!("Failed to connect to daemon at {base_url}: {e}"))?;

    response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Failed to parse daemon response: {e}"))
}

// --- Daemon (HTTP Server + Event Loop) ---

async fn run_daemon(wallet: Arc<Wallet>, host: &str, port: u16, webhooks: &[String]) {
    let app = Router::new()
        .route("/balance", get(handle_balance))
        .route("/receive", post(handle_receive))
        .route("/receive-offer", get(handle_receive_offer))
        .route("/send", post(handle_send))
        .route("/parse", post(handle_parse))
        .route("/transactions", get(handle_transactions))
        .route("/channels", get(handle_channels))
        .route("/info", get(handle_info))
        .route("/estimate-fee", post(handle_estimate_fee))
        .route("/lightning-address", get(handle_lightning_address))
        .route(
            "/register-lightning-address",
            post(handle_register_lightning_address),
        )
        .route("/get-event", get(handle_get_event))
        .route("/event-handled", post(handle_event_handled))
        .with_state(wallet.clone());

    let addr = format!("{host}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| {
            eprintln!("Failed to bind to {addr}: {e}");
            std::process::exit(1);
        });

    eprintln!("Daemon listening on {addr}");

    // Spawn HTTP server in background
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("HTTP server error: {e}");
        }
    });

    // Run event loop with webhooks
    run_event_loop(&wallet, webhooks).await;

    wallet.stop().await;
}

async fn run_event_loop(wallet: &Wallet, webhooks: &[String]) {
    let client = reqwest::Client::new();

    let hooks: Vec<(String, Option<String>)> = webhooks
        .iter()
        .map(|w| match w.split_once('|') {
            Some((url, token)) => (url.to_string(), Some(token.trim().to_string())),
            None => (w.clone(), None),
        })
        .collect();
    let has_webhooks = !hooks.is_empty();

    if has_webhooks {
        for (url, token) in &hooks {
            if token.is_some() {
                eprintln!("Webhook: {url} (auth: Bearer token)");
            } else {
                eprintln!("Webhook: {url}");
            }
        }
    } else {
        eprintln!("No webhooks configured, events will queue until consumed via get-event/event-handled or the HTTP API");
    }
    eprintln!("Press Ctrl+C to stop");

    loop {
        tokio::select! {
            event = wallet.next_event_async() => {
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                let value = serialize_event(&event, timestamp);

                for (url, token) in &hooks {
                    let client = client.clone();
                    let url = url.clone();
                    let body = value.clone();
                    let token = token.clone();
                    tokio::spawn(async move {
                        let max_retries = 3u32;
                        for attempt in 0..=max_retries {
                            let mut req = client.post(&url).json(&body);
                            if let Some(ref t) = token {
                                req = req.bearer_auth(t);
                            }
                            match req.send().await {
                                Ok(resp) if resp.status().is_success() => break,
                                Ok(resp) if resp.status().is_server_error() => {
                                    if attempt < max_retries {
                                        let delay = 1u64 << attempt;
                                        eprintln!("Webhook {url} returned {}, retrying in {delay}s...", resp.status());
                                        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                                    } else {
                                        eprintln!("Webhook {url} returned {} after {max_retries} retries, giving up", resp.status());
                                    }
                                }
                                Ok(resp) => {
                                    // 4xx client errors — don't retry
                                    eprintln!("Webhook {url} returned {}", resp.status());
                                    break;
                                }
                                Err(e) => {
                                    if attempt < max_retries {
                                        let delay = 1u64 << attempt;
                                        eprintln!("Webhook {url} failed: {e}, retrying in {delay}s...");
                                        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                                    } else {
                                        eprintln!("Webhook {url} failed after {max_retries} retries: {e}");
                                    }
                                }
                            }
                        }
                    });
                }

                eprintln!("[{timestamp}] {}", value["type"]);

                if has_webhooks {
                    let _ = wallet.event_handled();
                }
            }
            _ = tokio::signal::ctrl_c() => {
                eprintln!("Shutting down...");
                break;
            }
        }
    }
}

// --- HTTP Handlers ---

async fn handle_balance(State(wallet): State<Arc<Wallet>>) -> Json<serde_json::Value> {
    Json(match cmd_balance(&wallet).await {
        Ok(v) => v,
        Err(e) => json!({"error": e}),
    })
}

async fn handle_receive(
    State(wallet): State<Arc<Wallet>>,
    Json(body): Json<ReceiveRequest>,
) -> Json<serde_json::Value> {
    Json(match cmd_receive(&wallet, body.amount).await {
        Ok(v) => v,
        Err(e) => json!({"error": e}),
    })
}

async fn handle_receive_offer(State(wallet): State<Arc<Wallet>>) -> Json<serde_json::Value> {
    Json(match cmd_receive_offer(&wallet).await {
        Ok(v) => v,
        Err(e) => json!({"error": e}),
    })
}

async fn handle_send(
    State(wallet): State<Arc<Wallet>>,
    Json(body): Json<SendRequest>,
) -> Json<serde_json::Value> {
    Json(match cmd_send(&wallet, &body.payment, body.amount).await {
        Ok(v) => v,
        Err(e) => json!({"error": e}),
    })
}

async fn handle_parse(
    State(wallet): State<Arc<Wallet>>,
    Json(body): Json<PaymentRequest>,
) -> Json<serde_json::Value> {
    Json(match cmd_parse(&wallet, &body.payment).await {
        Ok(v) => v,
        Err(e) => json!({"error": e}),
    })
}

async fn handle_transactions(State(wallet): State<Arc<Wallet>>) -> Json<serde_json::Value> {
    Json(match cmd_transactions(&wallet).await {
        Ok(v) => v,
        Err(e) => json!({"error": e}),
    })
}

async fn handle_channels(State(wallet): State<Arc<Wallet>>) -> Json<serde_json::Value> {
    Json(match cmd_channels(&wallet) {
        Ok(v) => v,
        Err(e) => json!({"error": e}),
    })
}

async fn handle_info(State(wallet): State<Arc<Wallet>>) -> Json<serde_json::Value> {
    Json(match cmd_info(&wallet) {
        Ok(v) => v,
        Err(e) => json!({"error": e}),
    })
}

async fn handle_estimate_fee(
    State(wallet): State<Arc<Wallet>>,
    Json(body): Json<PaymentRequest>,
) -> Json<serde_json::Value> {
    Json(match cmd_estimate_fee(&wallet, &body.payment).await {
        Ok(v) => v,
        Err(e) => json!({"error": e}),
    })
}

async fn handle_lightning_address(State(wallet): State<Arc<Wallet>>) -> Json<serde_json::Value> {
    Json(match cmd_lightning_address(&wallet).await {
        Ok(v) => v,
        Err(e) => json!({"error": e}),
    })
}

async fn handle_register_lightning_address(
    State(wallet): State<Arc<Wallet>>,
    Json(body): Json<RegisterAddressRequest>,
) -> Json<serde_json::Value> {
    Json(
        match cmd_register_lightning_address(&wallet, &body.name).await {
            Ok(v) => v,
            Err(e) => json!({"error": e}),
        },
    )
}

async fn handle_get_event(State(wallet): State<Arc<Wallet>>) -> Json<serde_json::Value> {
    Json(match cmd_get_event(&wallet) {
        Ok(v) => v,
        Err(e) => json!({"error": e}),
    })
}

async fn handle_event_handled(State(wallet): State<Arc<Wallet>>) -> Json<serde_json::Value> {
    Json(match cmd_event_handled(&wallet) {
        Ok(v) => v,
        Err(e) => json!({"error": e}),
    })
}

// --- Wallet Command Implementations ---

async fn cmd_balance(wallet: &Wallet) -> Result<serde_json::Value, String> {
    let balance = wallet
        .get_balance()
        .await
        .map_err(|e| format!("Failed to get balance: {e:?}"))?;
    Ok(json!({
        "trusted_sats": balance.trusted.sats_rounding_up(),
        "lightning_sats": balance.lightning.sats_rounding_up(),
        "pending_sats": balance.pending_balance.sats_rounding_up(),
        "available_sats": balance.available_balance().sats_rounding_up(),
    }))
}

async fn cmd_receive(
    wallet: &Wallet,
    amount_sats: Option<u64>,
) -> Result<serde_json::Value, String> {
    let amount = match amount_sats {
        Some(sats) => Some(Amount::from_sats(sats).map_err(|_| "Invalid amount".to_string())?),
        None => None,
    };

    let uri = wallet
        .get_single_use_receive_uri(amount)
        .await
        .map_err(|e| format!("Failed to generate receive URI: {e:?}"))?;

    Ok(json!({
        "invoice": uri.invoice.to_string(),
        "address": uri.address.as_ref().map(|a| a.to_string()),
        "amount_sats": uri.amount.map(|a| a.sats_rounding_up()),
        "full_uri": uri.to_string(),
        "from_trusted": uri.from_trusted,
    }))
}

async fn cmd_receive_offer(wallet: &Wallet) -> Result<serde_json::Value, String> {
    let offer = wallet
        .get_reusable_receive_uri()
        .await
        .map_err(|e| format!("Failed to get reusable URI: {e:?}"))?;
    Ok(json!({
        "offer": offer,
    }))
}

async fn cmd_send(
    wallet: &Wallet,
    payment: &str,
    amount_sats: Option<u64>,
) -> Result<serde_json::Value, String> {
    let amount = match amount_sats {
        Some(sats) => Some(Amount::from_sats(sats).map_err(|_| "Invalid amount".to_string())?),
        None => None,
    };

    let instructions = wallet
        .parse_payment_instructions(payment)
        .await
        .map_err(|e| format!("Failed to parse payment: {e:?}"))?;

    let payment_info = PaymentInfo::build(instructions, amount)
        .map_err(|e| format!("Failed to build payment info: {e:?}"))?;

    let payment_id = wallet
        .pay(&payment_info)
        .await
        .map_err(|e| format!("Failed to send payment: {e:?}"))?;

    Ok(json!({
        "payment_id": payment_id.to_string(),
        "amount_sats": payment_info.amount().sats_rounding_up(),
        "status": "initiated",
    }))
}

async fn cmd_parse(wallet: &Wallet, payment: &str) -> Result<serde_json::Value, String> {
    let instructions = wallet
        .parse_payment_instructions(payment)
        .await
        .map_err(|e| format!("Failed to parse payment: {e:?}"))?;
    Ok(json!({
        "parsed": format!("{instructions:?}"),
    }))
}

async fn cmd_transactions(wallet: &Wallet) -> Result<serde_json::Value, String> {
    let transactions = wallet
        .list_transactions()
        .await
        .map_err(|e| format!("Failed to list transactions: {e:?}"))?;

    let txs: Vec<serde_json::Value> = transactions
        .iter()
        .map(|tx| {
            json!({
                "id": tx.id.to_string(),
                "status": format!("{:?}", tx.status),
                "outbound": tx.outbound,
                "amount_sats": tx.amount.map(|a| a.sats_rounding_up()),
                "fee_sats": tx.fee.map(|a| a.sats_rounding_up()),
                "payment_type": format!("{:?}", tx.payment_type),
                "timestamp": tx.time_since_epoch.as_secs(),
            })
        })
        .collect();

    Ok(json!({
        "count": txs.len(),
        "transactions": txs,
    }))
}

fn cmd_channels(wallet: &Wallet) -> Result<serde_json::Value, String> {
    let channels = wallet.channels();
    let chans: Vec<serde_json::Value> = channels
        .iter()
        .map(|ch| {
            json!({
                "channel_id": ch.channel_id.to_string(),
                "counterparty_node_id": ch.counterparty_node_id.to_string(),
                "funding_txo": ch.funding_txo.map(|t| t.to_string()),
                "is_channel_ready": ch.is_channel_ready,
                "is_usable": ch.is_usable,
                "inbound_capacity_sats": ch.inbound_capacity_msat / 1_000,
                "outbound_capacity_sats": ch.outbound_capacity_msat / 1_000,
                "channel_value_sats": ch.channel_value_sats,
            })
        })
        .collect();

    Ok(json!({
        "count": chans.len(),
        "channels": chans,
    }))
}

fn cmd_info(wallet: &Wallet) -> Result<serde_json::Value, String> {
    let tunables = wallet.get_tunables();
    Ok(json!({
        "node_id": wallet.node_id().to_string(),
        "lsp_connected": wallet.is_connected_to_lsp(),
        "tunables": {
            "trusted_balance_limit_sats": tunables.trusted_balance_limit.sats_rounding_up(),
            "rebalance_min_sats": tunables.rebalance_min.sats_rounding_up(),
            "onchain_receive_threshold_sats": tunables.onchain_receive_threshold.sats_rounding_up(),
            "enable_amountless_receive_on_chain": tunables.enable_amountless_receive_on_chain,
        },
    }))
}

async fn cmd_estimate_fee(wallet: &Wallet, payment: &str) -> Result<serde_json::Value, String> {
    let instructions = wallet
        .parse_payment_instructions(payment)
        .await
        .map_err(|e| format!("Failed to parse payment for fee estimation: {e:?}"))?;

    let fee = wallet.estimate_fee(&instructions).await;
    Ok(json!({
        "estimated_fee_sats": fee.sats_rounding_up(),
    }))
}

async fn cmd_lightning_address(wallet: &Wallet) -> Result<serde_json::Value, String> {
    let address = wallet
        .get_lightning_address()
        .await
        .map_err(|e| format!("Failed to get lightning address: {e:?}"))?;
    Ok(json!({
        "lightning_address": address,
    }))
}

async fn cmd_register_lightning_address(
    wallet: &Wallet,
    name: &str,
) -> Result<serde_json::Value, String> {
    wallet
        .register_lightning_address(name.to_string())
        .await
        .map_err(|e| format!("Failed to register lightning address: {e:?}"))?;

    let address = wallet
        .get_lightning_address()
        .await
        .map_err(|e| format!("Failed to get lightning address: {e:?}"))?;

    Ok(json!({
        "registered": true,
        "lightning_address": address,
    }))
}

fn cmd_get_event(wallet: &Wallet) -> Result<serde_json::Value, String> {
    match wallet.next_event() {
        Some(event) => {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            Ok(serialize_event(&event, timestamp))
        }
        None => Ok(json!({ "event": null })),
    }
}

fn cmd_event_handled(wallet: &Wallet) -> Result<serde_json::Value, String> {
    wallet
        .event_handled()
        .map_err(|_| "Failed to mark event as handled".to_string())?;
    Ok(json!({ "ok": true }))
}

fn serialize_event(event: &Event, timestamp: u64) -> serde_json::Value {
    match event {
        Event::PaymentSuccessful {
            payment_id,
            payment_hash,
            payment_preimage,
            fee_paid_msat,
        } => json!({
            "type": "payment_successful",
            "timestamp": timestamp,
            "payment_id": payment_id.to_string(),
            "payment_hash": format!("{payment_hash:?}"),
            "payment_preimage": format!("{payment_preimage:?}"),
            "fee_paid_msat": fee_paid_msat,
        }),
        Event::PaymentFailed {
            payment_id,
            payment_hash,
            reason,
        } => json!({
            "type": "payment_failed",
            "timestamp": timestamp,
            "payment_id": payment_id.to_string(),
            "payment_hash": payment_hash.map(|h| format!("{h:?}")),
            "reason": reason.map(|r| format!("{r:?}")),
        }),
        Event::PaymentReceived {
            payment_id,
            payment_hash,
            amount_msat,
            custom_records,
            lsp_fee_msats,
        } => json!({
            "type": "payment_received",
            "timestamp": timestamp,
            "payment_id": payment_id.to_string(),
            "payment_hash": format!("{payment_hash:?}"),
            "amount_msat": amount_msat,
            "amount_sats": amount_msat / 1000,
            "custom_records_count": custom_records.len(),
            "lsp_fee_msats": lsp_fee_msats,
        }),
        Event::OnchainPaymentReceived {
            payment_id,
            txid,
            amount_sat,
            status,
        } => json!({
            "type": "onchain_payment_received",
            "timestamp": timestamp,
            "payment_id": payment_id.to_string(),
            "txid": txid.to_string(),
            "amount_sat": amount_sat,
            "status": format!("{status:?}"),
        }),
        Event::ChannelOpened {
            channel_id,
            user_channel_id,
            counterparty_node_id,
            funding_txo,
        } => json!({
            "type": "channel_opened",
            "timestamp": timestamp,
            "channel_id": channel_id.to_string(),
            "user_channel_id": format!("{user_channel_id:?}"),
            "counterparty_node_id": counterparty_node_id.to_string(),
            "funding_txo": funding_txo.to_string(),
        }),
        Event::ChannelClosed {
            channel_id,
            user_channel_id,
            counterparty_node_id,
            reason,
        } => json!({
            "type": "channel_closed",
            "timestamp": timestamp,
            "channel_id": channel_id.to_string(),
            "user_channel_id": format!("{user_channel_id:?}"),
            "counterparty_node_id": counterparty_node_id.to_string(),
            "reason": reason.as_ref().map(|r| format!("{r:?}")),
        }),
        Event::RebalanceInitiated {
            trigger_payment_id,
            trusted_rebalance_payment_id,
            amount_msat,
        } => json!({
            "type": "rebalance_initiated",
            "timestamp": timestamp,
            "trigger_payment_id": trigger_payment_id.to_string(),
            "trusted_rebalance_payment_id": trusted_rebalance_payment_id.to_lower_hex_string(),
            "amount_msat": amount_msat,
        }),
        Event::RebalanceSuccessful {
            trigger_payment_id,
            trusted_rebalance_payment_id,
            ln_rebalance_payment_id,
            amount_msat,
            fee_msat,
        } => json!({
            "type": "rebalance_successful",
            "timestamp": timestamp,
            "trigger_payment_id": trigger_payment_id.to_string(),
            "trusted_rebalance_payment_id": trusted_rebalance_payment_id.to_lower_hex_string(),
            "ln_rebalance_payment_id": ln_rebalance_payment_id.to_lower_hex_string(),
            "amount_msat": amount_msat,
            "fee_msat": fee_msat,
        }),
        Event::SplicePending {
            channel_id,
            user_channel_id,
            counterparty_node_id,
            new_funding_txo,
        } => json!({
            "type": "splice_pending",
            "timestamp": timestamp,
            "channel_id": channel_id.to_string(),
            "user_channel_id": format!("{user_channel_id:?}"),
            "counterparty_node_id": counterparty_node_id.to_string(),
            "new_funding_txo": new_funding_txo.to_string(),
        }),
    }
}
