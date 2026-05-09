pub fn get_password(service: &str, account: &str) -> Result<String, Box<dyn std::error::Error>> {
    use keyring::Entry;

    let entry = Entry::new(service, account)?;
    let password = entry.get_password()?;
    Ok(password)
}

pub fn set_password(service: &str, account: &str, password: &str) -> Result<(), Box<dyn std::error::Error>> {
    use keyring::Entry;

    let entry = Entry::new(service, account)?;
    entry.set_password(password)?;
    Ok(())
}

pub fn delete_password(service: &str, account: &str) -> Result<(), Box<dyn std::error::Error>> {
    use keyring::Entry;

    let entry = Entry::new(service, account)?;
    entry.delete_password()?;
    Ok(())
}
