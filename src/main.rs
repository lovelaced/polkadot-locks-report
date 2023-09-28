use std::fs::File;
use std::io::{BufRead, BufReader};
use subxt::dynamic::{At, Value, DecodedValue};
use subxt::{OnlineClient, PolkadotConfig};
use subxt::utils;
use std::str::FromStr;

#[subxt::subxt(runtime_metadata_path = "./artifacts/polkadot_metadata_small.scale")]
pub mod polkadot {}

async fn fetch_and_print_balance(api: &OnlineClient<PolkadotConfig>, key: utils::AccountId32) -> Result<(), Box<dyn std::error::Error>> {
    // Use static methods to create the storage query
    let storage_query = polkadot::storage().balances().account(key);

    // Fetching the storage data
    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            // Assuming value is already of the correct type, you can directly process it
            println!("[Decoded Data for balances.account] {:?}", value);
        },
        Ok(None) => println!("[locks] Not found for address in balances.account"),
        Err(e) => {
            eprintln!("[Error] Fetching failed for balances.account: {}", e);
            return Err(e.into());
        }
    };

    Ok(())
}

async fn fetch_and_print_locks(api: &OnlineClient<PolkadotConfig>, key: utils::AccountId32) -> Result<(), Box<dyn std::error::Error>> {
    // Use static methods to create the storage query
    let storage_query = polkadot::storage().balances().locks(key);

    // Fetching the storage data
    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            // Assuming value is already of the correct type, you can directly process it
            println!("[Decoded Data for balances.locks] {:?}", value);
        },
        Ok(None) => println!("[locks] Not found for address in balances.locks"),
        Err(e) => {
            eprintln!("[Error] Fetching failed for balances.locks: {}", e);
            return Err(e.into());
        }
    };

    Ok(())
}

async fn fetch_and_print_conviction_locks(api: &OnlineClient<PolkadotConfig>, key: utils::AccountId32) -> Result<(), Box<dyn std::error::Error>> {
    // Use static methods to create the storage query
    let storage_query = polkadot::storage().conviction_voting().class_locks_for(key);

    // Fetching the storage data
    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            // Assuming value is already of the correct type, you can directly process it
            println!("[Decoded Data for conviction locks] {:?}", value);
        },
        Ok(None) => println!("[voting] Not found for address in conviction locks"),
        Err(e) => {
            eprintln!("[Error] Fetching failed for conviction locks: {}", e);
            return Err(e.into());
        }
    };

    Ok(())
}

async fn fetch_and_print_voting(api: &OnlineClient<PolkadotConfig>, key: utils::AccountId32) -> Result<(), Box<dyn std::error::Error>> {
    // Use static methods to create the storage query
    let lock_class: u16 = 16;
    let storage_query = polkadot::storage().conviction_voting().voting_for(key, lock_class);

    // Fetching the storage data
    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            // Assuming value is already of the correct type, you can directly process it
            println!("[Decoded Data for votes] {:?}", value);
        },
        Ok(None) => println!("[voting] Not found for address in conviction votes"),
        Err(e) => {
            eprintln!("[Error] Fetching failed for conviction votes: {}", e);
            return Err(e.into());
        }
    };

    Ok(())
}
async fn fetch_and_print_vesting(api: &OnlineClient<PolkadotConfig>, key: utils::AccountId32) -> Result<(), Box<dyn std::error::Error>> {
    // Use static methods to create the storage query
    let storage_query = polkadot::storage().vesting().vesting(key);

    // Fetching the storage data
    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            // Assuming value is already of the correct type, you can directly process it
            println!("[Decoded Data for vesting] {:?}", value);
        },
        Ok(None) => println!("[vesting] Not found for address in vesting"),
        Err(e) => {
            eprintln!("[Error] Fetching failed for vesting: {}", e);
            return Err(e.into());
        }
    };

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api = connect_to_polkadot_node().await?;
    let addresses = read_addresses_from_file("addresses.txt")?;

    for address in &addresses {
        process_address(&api, &address).await?;
    }

    println!("\n[Completion] Finished processing all addresses.");
    Ok(())
}

async fn connect_to_polkadot_node() -> Result<OnlineClient<PolkadotConfig>, Box<dyn std::error::Error>> {
    println!("[Connection] Attempting to connect to 'wss://rpc.polkadot.io:443'...");
    OnlineClient::<PolkadotConfig>::from_url("wss://rpc.polkadot.io:443").await.map_err(Into::into)
}

fn read_addresses_from_file(path: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    BufReader::new(File::open(path)?).lines().collect::<Result<_, _>>().map_err(Into::into)
}

async fn process_address(
    api: &OnlineClient<PolkadotConfig>,
    address: &str
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n[Processing] Address: {}", address);
    let public_key_bytes = utils::AccountId32::from_str(address)?;

    println!("[Balance] Fetching general balance...");
    if let Err(e) = fetch_and_print_balance(&api, public_key_bytes.clone()).await {
        eprintln!("[Error] Failed to fetch balance: {}", e);
    }
    if let Err(e) = fetch_and_print_locks(&api, public_key_bytes.clone()).await {
        eprintln!("[Error] Failed to fetch locked balance: {}", e);
    }

    println!("[Vesting Balance] Fetching...");
    if let Err(e) = fetch_and_print_vesting(&api, public_key_bytes.clone()).await {
        eprintln!("[Error] Failed to fetch vesting balance: {}", e);
    }

    println!("[Conviction Voting] Fetching...");
    if let Err(e) = fetch_and_print_conviction_locks(&api, public_key_bytes.clone()).await {
        eprintln!("[Error] Failed to fetch conviction: {}", e);
    }
//    println!("[Conviction Voting] Fetching...");
//    if let Err(e) = fetch_and_print_voting(&api, public_key_bytes.clone()).await {
//        eprintln!("[Error] Failed to fetch votes: {}", e);
//    }
  Ok(())
}

