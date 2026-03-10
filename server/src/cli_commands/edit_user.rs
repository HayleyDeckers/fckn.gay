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

    const BACKOFFS: &[u64] = &[1, 2, 4, 16];

    let mut succeeded = 0u32;
    let mut failed = 0u32;
    let mut dangling_old = 0u32;
    let mut failed_names: Vec<String> = Vec::new();

    for rec in &db_records {
        let new_name = rename_record_name(&rec.record.name, old_username, new_username, suffix);
        let mut new_record = rec.record.clone();
        new_record.name = new_name;

        eprintln!("  📝 {} -> {}", rec.record.name, new_record.name);

        // 1. add new record upstream (fatal — nothing mutated yet, safe to skip)
        let mut add_upstream = dns.add_record(new_record.clone()).await;
        for &secs in BACKOFFS {
            if add_upstream.is_ok() {
                break;
            }
            eprintln!("    ⏳ retrying add upstream after {secs}s...");
            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
            add_upstream = dns.add_record(new_record.clone()).await;
        }
        let new_key = match add_upstream {
            Ok(key) => key,
            Err(e) => {
                eprintln!("    ❌ gave up adding '{}' upstream: {e}", new_record.name);
                failed += 1;
                failed_names.push(new_record.name.clone());
                continue;
            }
        };

        // 2. track new record in DB (on failure, roll back step 1)
        let new_key_str = new_key.to_string();
        let mut add_db = user_db
            .add_dns_record(user_id, new_record.clone(), new_key_str.clone())
            .await;
        for &secs in BACKOFFS {
            if add_db.is_ok() {
                break;
            }
            eprintln!("    ⏳ retrying add DB record after {secs}s...");
            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
            add_db = user_db
                .add_dns_record(user_id, new_record.clone(), new_key_str.clone())
                .await;
        }
        if add_db.is_err() {
            eprintln!(
                "    ❌ couldn't track '{}' in DB -- rolling back upstream add",
                new_record.name
            );
            if let Err(e) = dns.delete_record(new_key.clone()).await {
                eprintln!(
                    "    ⚠️  rollback failed too: {e} -- upstream record '{}' is now untracked 💀",
                    new_record.name
                );
                dangling_old += 1;
            }
            failed += 1;
            failed_names.push(new_record.name.clone());
            continue;
        }

        // 3. delete old record upstream (non-fatal — new record is already live + tracked)
        let old_key: fckn_gay_dns::Key = match rec.provider_key.parse() {
            Ok(k) => k,
            Err(e) => {
                eprintln!(
                    "    ⚠️  can't parse old provider key '{}': {e} -- old record may linger",
                    rec.provider_key
                );
                dangling_old += 1;
                succeeded += 1;
                continue;
            }
        };
        let mut del_old_upstream = dns.delete_record(old_key.clone()).await;
        for &secs in BACKOFFS {
            if del_old_upstream.is_ok() {
                break;
            }
            eprintln!("    ⏳ retrying delete old upstream after {secs}s...");
            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
            del_old_upstream = dns.delete_record(old_key.clone()).await;
        }
        if let Err(e) = del_old_upstream {
            eprintln!(
                "    ⚠️  couldn't delete old upstream record '{}': {e} -- may linger",
                rec.provider_key
            );
            dangling_old += 1;
        }

        // 4. remove old record from DB (non-fatal — it's just stale bookkeeping)
        let mut del_old_db = user_db.delete_dns_record(user_id, rec.id).await;
        for &secs in BACKOFFS {
            if del_old_db.is_ok() {
                break;
            }
            eprintln!("    ⏳ retrying delete old DB record after {secs}s...");
            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
            del_old_db = user_db.delete_dns_record(user_id, rec.id).await;
        }
        if let Err(e) = del_old_db {
            eprintln!(
                "    ⚠️  couldn't clean up old DB record for '{}': {e} -- stale entry remains",
                rec.record.name
            );
        }

        succeeded += 1;
        eprintln!("  ✅ renamed record -> {}", new_record.name);
    }

    eprintln!("\n  === rename summary ===");
    eprintln!("  records renamed:   {succeeded}");
    if dangling_old > 0 {
        eprintln!("  old records left:  {dangling_old} (may need manual cleanup)");
    }
    if failed > 0 {
        eprintln!("  records failed:    {failed}");
        for name in &failed_names {
            eprintln!("    ❌ {name}");
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
