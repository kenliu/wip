use secret_service::{EncryptionType, SecretService};
use std::collections::HashMap;

const SERVICE: &str = "net.kenliu.wip";

pub async fn get_password(account: &str) -> Result<String, Box<dyn std::error::Error>> {
    let ss = SecretService::connect(EncryptionType::Dh).await?;
    let attrs = HashMap::from([("service", SERVICE), ("account", account)]);
    let results = ss.search_items(attrs).await?;

    let item = match results.unlocked.first() {
        Some(item) => item,
        None => {
            let locked = results.locked.first().ok_or("No matching secret found")?;
            locked.unlock().await?;
            locked
        }
    };

    let secret = item.get_secret().await?;
    Ok(String::from_utf8(secret)?)
}

pub async fn set_password(account: &str, password: &str) -> Result<(), Box<dyn std::error::Error>> {
    let ss = SecretService::connect(EncryptionType::Dh).await?;
    let collection = ss.get_default_collection().await?;
    collection.unlock().await?;

    let attrs = HashMap::from([("service", SERVICE), ("account", account)]);
    collection
        .create_item(
            &format!("{}/{}", SERVICE, account),
            attrs,
            password.as_bytes(),
            true, // replace existing item with same attributes
            "text/plain",
        )
        .await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn delete_password(account: &str) -> Result<(), Box<dyn std::error::Error>> {
    let ss = SecretService::connect(EncryptionType::Dh).await?;
    let attrs = HashMap::from([("service", SERVICE), ("account", account)]);
    let results = ss.search_items(attrs).await?;
    for item in results.unlocked.iter().chain(results.locked.iter()) {
        item.delete().await?;
    }
    Ok(())
}
