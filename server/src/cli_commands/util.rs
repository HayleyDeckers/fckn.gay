pub fn generate_random_password() -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%&*";
    let mut bytes = [0u8; 32];

    getrandom::fill(&mut bytes).expect("getrandom failed -- is the OS entropy source broken?");
    // biased as 255 does not evenly divide into CHARSET.len(), but good enough for our purposes
    bytes
        .iter()
        .map(|b| CHARSET[(*b as usize) % CHARSET.len()] as char)
        .collect()
}
