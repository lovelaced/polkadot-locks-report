use std::fs::File;
use std::io::{BufRead, BufReader};
use subxt::dynamic::{At, Value, DecodedValueThunk};
use subxt::{OnlineClient, PolkadotConfig};
use subxt::utils;
use std::str::FromStr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a new API client, configured to talk to Polkadot nodes.
    let api = OnlineClient::<PolkadotConfig>::from_url("wss://rpc.polkadot.io:443").await?;

    // Read SS58 formatted addresses from a file.
    let file = File::open("addresses.txt")?;
    let reader = BufReader::new(file);
    let addresses: Vec<String> = reader.lines().filter_map(Result::ok).collect();

    // Iterate through each address, convert to required format, and fetch locked balances.
    for address in addresses {
        let public_key_bytes: utils::AccountId32 = utils::AccountId32::from_str(&address)?;
        let key: Value = Value::from_bytes(&public_key_bytes);

        // Build a dynamic storage query to access the locked balances.
        let storage_query = subxt::dynamic::storage("Balances", "Locks", vec![key]);

        // Fetch the lock information dynamically.
        let result: Option<DecodedValueThunk> = api
            .storage()
            .at_latest()
            .await?
            .fetch(&storage_query)
            .await?;
        
        let value = result.unwrap().to_value()?;

        // Print the locked balance for each account.
        println!("Locked balance for account ({}): {:?}", &address, value.at("data").at("free"));
    }

    Ok(())
}

