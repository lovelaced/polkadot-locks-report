use std::fs::File;
use std::io::{BufRead, BufReader};
use subxt::dynamic::{At, Value, DecodedValue};
use subxt::{OnlineClient, PolkadotConfig};
use subxt::utils;
use std::str::FromStr;

fn process_locks_data(decoded_locks: &DecodedValue) {
// Navigate to the main array of locks
if let Some(locks_array) = decoded_locks.at("value").at("value").at(0) {
    let mut index = 0;

    // Iterate over each lock in the locks_array
    while let Some(lock_data) = locks_array.at(index) {
        // Extract the "id" which is an array of Value items.
        if let Some(id_comp) = lock_data.at("id") {
            let mut char_index = 0;
            let mut id_str = String::new();

            // Iterate over the ID components and assemble the string
            while let Some(char_val) = id_comp.at(char_index) {
                if let Some(val) = char_val.as_u128() {
                    id_str.push(val as u8 as char); // Convert U128 to char
                }
                char_index += 1;
            }

            println!("Lock id: {}", id_str);
        }

        // Extract the amount from the lock data
        if let Some(amount_val) = lock_data.at("amount") {
            if let Some(amount) = amount_val.as_u128() {
                println!("Amount: {}", amount);
            }
        }

        // Extract the reasons from the lock data
        if let Some(reasons_value) = lock_data.at("reasons") {
            // If the reasons_value is a variant, its exact structure might be different.
            // You may need to further process this value based on your requirements.
            println!("Reasons: {:?}", reasons_value);
        }

        index += 1; // Move to the next lock in the array
    }
} else {
    println!("Unexpected lock data structure!");
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
                    println!("[Decoded Data for {}.{}] {:?}", module, item, decoded_value);

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

