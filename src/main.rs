use std::fs::File;
use std::io::{BufRead, BufReader};
use subxt::dynamic::{At, Value, DecodedValue};
use subxt::{OnlineClient, PolkadotConfig};
use subxt::utils;
use std::str::FromStr;

fn process_locks_data(decoded_locks_data: &DecodedValue) {

    if let Some(outer_value) = decoded_locks_data.at(0) {
        let mut index = 0;
        const MAX_ITERATIONS: usize = 1000;  // Safety precaution
        
        while index < MAX_ITERATIONS {
            let lock_data = outer_value.at(index);
            match lock_data {
                None => {
                    //println!("No lock data found for index {}. Terminating loop.", index);
                    break;  // No more locks to process
                },
                Some(lock) => {

                    // Extract the "id"
                    if let Some(id_comp) = lock.at("id") {

                        let mut char_index = 0;
                        let mut id_chars = Vec::new();

                        while let Some(char_value) = id_comp.at(char_index).and_then(|v| v.as_u128()) {
                            id_chars.push(char_value as u8 as char);
                            char_index += 1;
                        }

                        if !id_chars.is_empty() {
                            let id_str: String = id_chars.into_iter().collect();
                            println!("Lock id: {}", id_str);
                        } else {
                            println!("No characters found in ID component.");
                        }

                    } else {
                        println!("Failed to extract ID for a lock at index {}.", index);
                    }

                    // Extract the amount
                    if let Some(amount) = lock.at("amount").and_then(|amt| amt.as_u128()) {
                        println!("Amount for lock at index {}: {}", index, amount);
                    } else {
                        println!("Failed to extract amount for a lock at index {}.", index);
                    }

                    // Extract the reasons
                    if let Some(reasons_value) = lock.at("reasons") {
                        //println!("Reasons for lock at index {}: {:?}", index, reasons_value);
                    } else {
                        println!("Failed to extract reasons for a lock at index {}.", index);
                    }
                }
            }
            index += 1;
        }
        
        if index >= MAX_ITERATIONS {
            println!("Warning: Reached maximum iteration count. Please check the data structure.");
        }

    } else {
        println!("Unexpected lock structure. Outer value not found.");
    }
}

async fn fetch_and_print_storage_data(api: &OnlineClient<PolkadotConfig>, module: &str, item: &str, key: Value) -> Result<(), Box<dyn std::error::Error>> {
    let storage_query = subxt::storage::dynamic(module, item, vec![key]);

    // Fetching the storage data
    match api.storage().at_latest().await?.fetch(&storage_query).await {
        Ok(Some(value)) => {
            match value.to_value() {
                Ok(decoded_value) => {
                    // Printing the decoded data
                //    println!("[Decoded Data for {}.{}] {:?}", module, item, decoded_value);

                    // If you want to access specific fields within the decoded data, you can do so:
                    process_locks_data(&decoded_value);
                },
                Err(e) => {
                    eprintln!("[Error] Converting to value failed for {}.{}: {}", module, item, e);
                }
            }
        },
        Ok(None) => println!("[{}] Not found for address in {}.{}", item, module, item),
        Err(e) => {
            eprintln!("[Error] Fetching failed for {}.{}: {}", module, item, e);
            return Err(e.into());
        }
    };

    Ok(())
}


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

    println!("[Balance] Fetching general balance...");
    if let Err(e) = fetch_and_print_storage_data(&api, "Balances", "Account", key.clone()).await {
        eprintln!("[Error] Failed to fetch balance: {}", e);
        continue;
    }

    println!("[Locked Balance] Fetching...");
    if let Err(e) = fetch_and_print_storage_data(&api, "Balances", "Locks", key).await {
        eprintln!("[Error] Failed to fetch locked balance: {}", e);
        continue;
    }
}

    println!("\n[Completion] Finished processing all addresses.");
    Ok(())
}

