use anyhow::Result;
use fckn_gay_dns::{Dns, Interface as DnsInterface};
use fckn_gay_user_database::{
    Database as UserDatabase, Interface as UserDatabaseInterface, PasswordHash, UserState,
};

use super::util::generate_random_password;
use crate::interfaces::PublicSuffix;

pub struct EditUserOpts {
    pub email: Option<String>,
    pub state: Option<UserState>,
    pub reset_password: bool,
    pub rename: Option<String>,
    pub dry_run: bool,
}

pub async fn run(
    dns: &Dns,
    user_db: &UserDatabase,
    suffix: &PublicSuffix,
    username: &str,
    opts: EditUserOpts,
) -> Result<()> {
    let entry = user_db
        .get_user_by_username_or_email(username)
        .await
        .map_err(|e| anyhow::anyhow!("failed to look up user '{username}': {e}"))?
        .ok_or_else(|| anyhow::anyhow!("user '{username}' not found 💀"))?;

    let user_id = entry.id;

    let has_changes = opts.email.is_some()
        || opts.state.is_some()
        || opts.reset_password
        || opts.rename.is_some();

    if !has_changes {
        eprintln!("📋 user info for '{username}':");
        eprintln!("  id:         {}", entry.id);
        eprintln!("  username:   {}", entry.username);
        eprintln!("  email:      {}", entry.email);
        eprintln!("  state:      {}", entry.state);
        eprintln!("  created_at: {}", entry.created_at);
        eprintln!(
            "  last_login: {}",
            entry
                .last_login
                .map_or("never".to_string(), |t| t.to_string())
        );
        return Ok(());
    }

    if opts.dry_run {
        eprintln!("🧪 DRY RUN -- no changes will be made");
    }

    // -- email --
    if let Some(ref new_email) = opts.email {
        if opts.dry_run {
            eprintln!("📧 would update email: {} -> {new_email}", entry.email);
        } else {
            user_db
                .update_user_email(user_id, new_email)
                .await
                .map_err(|e| anyhow::anyhow!("failed to update email: {e}"))?;
            eprintln!("📧 updated email: {} -> {new_email}", entry.email);
        }
    }

    // -- state --
    if let Some(new_state) = opts.state {
        if opts.dry_run {
            eprintln!("🔄 would update state: {} -> {new_state}", entry.state);
        } else {
            user_db
                .update_user_state(user_id, new_state)
                .await
                .map_err(|e| anyhow::anyhow!("failed to update state: {e}"))?;
            eprintln!("🔄 updated state: {} -> {new_state}", entry.state);
        }
    }

    // -- password reset --
    if opts.reset_password {
        if opts.dry_run {
            eprintln!("🔑 would reset password (new password not generated in dry run)");
        } else {
            let new_pw = generate_random_password();
            user_db
                .update_user_password(user_id, PasswordHash::new(&new_pw))
                .await
                .map_err(|e| anyhow::anyhow!("failed to reset password: {e}"))?;
            eprintln!("🔑 password reset! new password: {new_pw}");
        }
    }

    // -- rename (the spicy one 🌶️) --
    if let Some(ref new_username) = opts.rename {
        rename_user(
            dns,
            user_db,
            suffix,
            user_id,
            &entry.username,
            new_username,
            opts.dry_run,
        )
        .await?;
    }

    if opts.dry_run {
        eprintln!("\n(dry run -- nothing was actually changed)");
    } else {
        eprintln!("\n✨ done!");
    }

    Ok(())
}

/// Rename a user: update username in DB + migrate all their DNS records upstream.
///
/// Since Porkbun doesn't support updating records in-place, we do delete + add per record.
/// Failed upstream adds go to a retry queue with exponential backoff (1s, 2s, 4s).
async fn rename_user(
    dns: &Dns,
    user_db: &UserDatabase,
    suffix: &PublicSuffix,
    user_id: fckn_gay_user_database::Uuid,
    old_username: &str,
    new_username: &str,
    dry_run: bool,
) -> Result<()> {
    eprintln!("\n🏷️  renaming '{old_username}' -> '{new_username}'...");

    let validation = fckn_gay_validation::validate_username(new_username);
    if !validation.is_valid() {
        anyhow::bail!(
            "username '{new_username}' is invalid: {}",
            validation.errors().join(", ")
        );
    }

    if !user_db.is_available(new_username).await {
        anyhow::bail!("username '{new_username}' is already taken, can't rename");
    }

    let db_records = user_db
        .get_user_dns_records(user_id)
        .await
        .map_err(|e| anyhow::anyhow!("failed to fetch DNS records: {e}"))?;

    if dry_run {
        eprintln!("  would update username in DB: {old_username} -> {new_username}");
        for rec in &db_records {
            let new_name = rename_record_name(&rec.record.name, old_username, new_username, suffix);
            eprintln!(
                "  would rename record: {} -> {new_name} ({} -> {})",
                rec.record.name, rec.record.record_type, rec.record.content,
            );
        }
        return Ok(());
    }

    // Step 1: update username in DB first
    user_db
        .update_username(user_id, new_username)
        .await
        .map_err(|e| anyhow::anyhow!("failed to update username in DB: {e}"))?;
    eprintln!("  ✅ updated username in DB");

    // Step 2: migrate DNS records
    if db_records.is_empty() {
        eprintln!("  no DNS records to rename");
        return Ok(());
    }

    struct PendingRename {
        old_provider_key: String,
        old_record_id: fckn_gay_user_database::DnsRecordId,
        new_record: fckn_gay_dns::Record,
    }

    let mut pending: Vec<PendingRename> = db_records
        .iter()
        .map(|rec| {
            let new_name = rename_record_name(&rec.record.name, old_username, new_username, suffix);
            let mut new_record = rec.record.clone();
            new_record.name = new_name;
            PendingRename {
                old_provider_key: rec.provider_key.clone(),
                old_record_id: rec.id,
                new_record,
            }
        })
        .collect();

    let mut succeeded = 0u32;
    let mut dangling_old = 0u32;
    let backoff_durations = [
        std::time::Duration::from_secs(1),
        std::time::Duration::from_secs(2),
        std::time::Duration::from_secs(4),
        std::time::Duration::from_secs(16),
    ];
    let max_attempts = 1 + backoff_durations.len(); // first try + retries

    for attempt in 0..max_attempts {
        if pending.is_empty() {
            break;
        }

        if attempt > 0 {
            let wait = backoff_durations[attempt - 1];
            eprintln!(
                "  ⏳ retrying {} failed record(s) after {}s...",
                pending.len(),
                wait.as_secs()
            );
            tokio::time::sleep(wait).await;
        }

        let mut still_failing = Vec::new();

        for rename in pending.drain(..) {
            // 1. add new record upstream
            let new_key = match dns.add_record(rename.new_record.clone()).await {
                Ok(key) => key,
                Err(e) => {
                    eprintln!(
                        "  ⚠️  failed to add '{}' upstream: {e}",
                        rename.new_record.name
                    );
                    still_failing.push(rename);
                    continue;
                }
            };

            // 2. delete old DB record
            if let Err(e) = user_db
                .delete_dns_record(user_id, rename.old_record_id)
                .await
            {
                eprintln!(
                    "  ⚠️  failed to delete old DB record for '{}': {e}",
                    rename.new_record.name
                );
            }

            // 3. add new DB record with new provider key
            if let Err(e) = user_db
                .add_dns_record(user_id, rename.new_record.clone(), new_key.to_string())
                .await
            {
                eprintln!(
                    "  ⚠️  failed to add new DB record for '{}': {e}",
                    rename.new_record.name
                );
            }

            // 4. delete old upstream record (non-fatal)
            let old_key: fckn_gay_dns::Key = match rename.old_provider_key.parse() {
                Ok(k) => k,
                Err(e) => {
                    eprintln!(
                        "  ⚠️  can't parse old provider key '{}': {e} -- old record may linger",
                        rename.old_provider_key
                    );
                    dangling_old += 1;
                    succeeded += 1;
                    continue;
                }
            };

            if let Err(e) = dns.delete_record(old_key).await {
                eprintln!(
                    "  ⚠️  failed to delete old upstream record '{}': {e} -- may linger",
                    rename.old_provider_key
                );
                dangling_old += 1;
            }

            succeeded += 1;
            eprintln!("  ✅ renamed record -> {}", rename.new_record.name);
        }

        pending = still_failing;
    }

    let failed = pending.len() as u32;
    eprintln!("\n  === rename summary ===");
    eprintln!("  records renamed:   {succeeded}");
    if dangling_old > 0 {
        eprintln!("  old records left:  {dangling_old} (may need manual cleanup)");
    }
    if failed > 0 {
        eprintln!("  records failed:    {failed}");
        for rename in &pending {
            eprintln!("    ❌ {}", rename.new_record.name);
        }
    }

    Ok(())
}

/// Replace the old username portion in a DNS record name with the new username.
///
/// e.g. `sub.alice.is.fckn.gay` with old=`alice`, new=`bob` -> `sub.bob.is.fckn.gay`
fn rename_record_name(
    record_name: &str,
    old_username: &str,
    new_username: &str,
    suffix: &PublicSuffix,
) -> String {
    let suffix_str = suffix.as_str();

    // The record name should be <prefix>.<old_username><suffix>
    // where <prefix> might be empty (just <old_username><suffix>)
    let old_label = format!("{old_username}{suffix_str}");
    if let Some(prefix) = record_name.strip_suffix(&old_label) {
        let new_label = format!("{new_username}{suffix_str}");
        format!("{prefix}{new_label}")
    } else {
        // fallback: only replace if old_username appears as a full dot-delimited label
        let dot_old = format!(".{old_username}.");
        if let Some(pos) = record_name.find(&dot_old) {
            format!(
                "{}.{new_username}.{}",
                &record_name[..pos],
                &record_name[pos + dot_old.len()..]
            )
        } else if let Some(rest) = record_name.strip_prefix(&format!("{old_username}.")) {
            format!("{new_username}.{rest}")
        } else {
            eprintln!(
                "  ⚠️  couldn't find '{old_username}' as a label in '{record_name}' -- leaving as-is"
            );
            record_name.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suffix(s: &str) -> PublicSuffix {
        PublicSuffix::new(s.to_string())
    }

    #[test]
    fn rename_simple() {
        assert_eq!(
            rename_record_name("alice.is.fckn.gay", "alice", "bob", &suffix(".is.fckn.gay")),
            "bob.is.fckn.gay"
        );
    }

    #[test]
    fn rename_nested() {
        assert_eq!(
            rename_record_name(
                "sub.alice.is.fckn.gay",
                "alice",
                "bob",
                &suffix(".is.fckn.gay")
            ),
            "sub.bob.is.fckn.gay"
        );
    }

    #[test]
    fn rename_wildcard_nested() {
        assert_eq!(
            rename_record_name(
                "*.alice.is.fckn.gay",
                "alice",
                "bob",
                &suffix(".is.fckn.gay")
            ),
            "*.bob.is.fckn.gay"
        );
    }
}
