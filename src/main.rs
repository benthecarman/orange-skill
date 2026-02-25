mod config;

use clap::{Parser, Subcommand};
use config::Config;
use orange_sdk::bitcoin_payment_instructions::amount::Amount;
use orange_sdk::{PaymentInfo, Wallet};
use serde_json::json;

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
}

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

    let result = match cli.command {
        Command::Balance => cmd_balance(&wallet).await,
        Command::Receive { amount } => cmd_receive(&wallet, amount).await,
        Command::ReceiveOffer => cmd_receive_offer(&wallet).await,
        Command::Send { payment, amount } => cmd_send(&wallet, &payment, amount).await,
        Command::Parse { payment } => cmd_parse(&wallet, &payment).await,
        Command::Transactions => cmd_transactions(&wallet).await,
        Command::Channels => cmd_channels(&wallet),
        Command::Info => cmd_info(&wallet),
        Command::EstimateFee { payment } => cmd_estimate_fee(&wallet, &payment).await,
    };

    match result {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
        Err(e) => {
            print_error(&e);
            std::process::exit(1);
        }
    }
}

fn print_error(msg: &str) {
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"error": msg})).unwrap()
    );
}

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
