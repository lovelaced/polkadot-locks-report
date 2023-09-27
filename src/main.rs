use std::fs::File;
use std::io::{BufRead, BufReader};
use subxt::dynamic::{At, Value, DecodedValueThunk};
use subxt::{OnlineClient, PolkadotConfig};
use subxt::utils;
use std::str::FromStr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Step 1: Connect to Polkadot node
    println!("[Connection] Attempting to connect to 'wss://rpc.polkadot.io:443'...");
    let api = match OnlineClient::<PolkadotConfig>::from_url("wss://rpc.polkadot.io:443").await {
        Ok(client) => {
            println!("[Connection] Connected to the Polkadot node.");
            client
        },
        Err(e) => {
            eprintln!("[Connection Error] {}", e);
            return Err(e.into());
        }
    };

    // Step 2: Open the addresses file
    println!("[File] Attempting to open 'addresses.txt'...");
    let file = match File::open("addresses.txt") {
        Ok(f) => {
            println!("[File] Opened successfully.");
            f
        },
        Err(e) => {
            eprintln!("[File Error] {}", e);
            return Err(e.into());
        }
    };

    // Step 3: Read the addresses from the file
    let reader = BufReader::new(file);
    let addresses: Vec<String> = reader.lines().filter_map(|line| {
        match line {
            Ok(address) => {
                println!("[Address] Found: {}", address);
                Some(address)
            },
            Err(e) => {
                eprintln!("[Address Error] {}", e);
                None
            }
        }
    }).collect();

    // Step 4: Process each address
    for address in &addresses {
        println!("\n[Processing] Address: {}", address);

        // Convert address to public key bytes
        let public_key_bytes = match utils::AccountId32::from_str(address) {
            Ok(pk) => {
                println!("[Conversion] Successful.");
                pk
            },
            Err(e) => {
                eprintln!("[Conversion Error] {}", e);
                continue;
            }
        };

        let key: Value = Value::from_bytes(&public_key_bytes);
// Fetch general balance
println!("[Balance] Fetching general balance...");
let balance_storage_query = subxt::storage::dynamic("Balances", "Account", vec![key.clone()]);
match api.storage().at_latest().await?.fetch(&balance_storage_query).await {
    Ok(Some(balance_value)) => {
        match balance_value.to_value() {
            Ok(val) => {
                println!("[Raw Balance Data] {:?}", val); // <-- This line prints the raw data
                println!("[Balance] Free: {:?}", val.at("data").at("free"));
            },
            Err(e) => {
                eprintln!("[Balance Error] Converting to value failed: {}", e);
            }
        }
    },
    Ok(None) => println!("[Balance] Not found for address."),
    Err(e) => {
        eprintln!("[Balance Error] Fetching failed: {}", e);
        continue;
    }
};

        // Fetch locked balance
        println!("[Locked Balance] Fetching...");
        let storage_query = subxt::dynamic::storage("Balances", "Locks", vec![key]);
        match api.storage().at_latest().await?.fetch(&storage_query).await {
            Ok(Some(value)) => {
                match value.to_value() {
                    Ok(val) => {
                        println!("[Raw Locks Data] {:?}", val); // <-- This line prints the raw data
                        println!("[Locked Balance] Free: {:?}", val.at("data").at("amount"));
                    },
                    Err(e) => eprintln!("[Locked Balance Error] {}", e),
                }
            },
            Ok(None) => println!("[Locked Balance] Not found for address."),
            Err(e) => {
                eprintln!("[Locked Balance Error] Fetching failed: {}", e);
                continue;
            }
        };
    }

    println!("\n[Completion] Finished processing all addresses.");
    Ok(())
}

