use std::{
    collections::{BTreeMap, HashSet},
    io::{self, BufRead, Write},
};

use anyhow::{Context, Result};
use fckn_gay_dns::{Dns, Interface as DnsInterface};
use fckn_gay_user_database::{Database as UserDatabase, Interface as UserDatabaseInterface};

use super::util::generate_random_password;
use crate::interfaces::PublicSuffix;

/// Given a record name like `admin.is.fckn.gay` and a suffix like `.is.fckn.gay`,
/// extracts the top-level username label (`admin`).
/// For nested subdomains like `sub.alice.is.fckn.gay`, the username is `alice`
/// (the label immediately before the suffix).
///
/// Returns `None` for wildcards (`*`), empty labels, and non-matching suffixes.
fn extract_username(record_name: &str, suffix: &str) -> Option<String> {
    let name = record_name.to_ascii_lowercase();
    let suffix_lower = suffix.to_ascii_lowercase();
    let without_suffix = name.strip_suffix(&suffix_lower)?;
    let username = without_suffix.rsplit('.').next()?;
    if username.is_empty() || username == "*" {
        return None;
    }
    Some(username.to_string())
}

/// Prompt for a line of input. Returns the trimmed input, or `None` if it was blank.
fn prompt(label: &str) -> Option<String> {
    eprint!("  {label}");
    io::stderr().flush().ok();
    let mut buf = String::new();
    io::stdin()
        .lock()
        .read_line(&mut buf)
        .expect("failed to read stdin");
    let trimmed = buf.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

const DEFAULT_EMAIL: &str = "im@fckn.gay";

struct MigrationStats {
    users_created: u32,
    users_deleted: u32,
    records_imported: u32,
    records_already_synced: u32,
    records_deleted: u32,
}

/// Run the DNS -> user_database migration.
///
/// 1. Fetches all records from the DNS provider
/// 2. Filters to records matching the public suffix
/// 3. Creates missing users (with interactive prompts)
/// 4. Syncs the records database to match upstream
/// 5. Reports records present in the DB but missing upstream
///
/// If `dry_run` is true, no changes are made -- we just report what *would* happen.
pub async fn run(
    dns: &Dns,
    user_db: &UserDatabase,
    suffix: &PublicSuffix,
    dry_run: bool,
) -> Result<()> {
    let suffix_str = suffix.as_str();
    if dry_run {
        eprintln!("🧪 DRY RUN -- no changes will be made");
    }
    eprintln!("🔍 fetching all records from DNS provider...");

    let upstream_records = dns
        .list_records()
        .await
        .context("failed to list records from DNS provider")?;

    eprintln!("   found {} total records upstream", upstream_records.len());

    // Filter to records belonging to our public suffix and group by username
    let mut by_user: BTreeMap<String, Vec<(String, fckn_gay_dns::Record)>> = BTreeMap::new();
    let mut skipped = 0u32;

    for (key, record) in &upstream_records {
        match extract_username(&record.name, suffix_str) {
            Some(username) => {
                by_user
                    .entry(username)
                    .or_default()
                    .push((key.to_string(), record.clone()));
            }
            None => {
                skipped += 1;
            }
        }
    }

    eprintln!(
        "   {n_users} users with {n_records} records match suffix '{suffix_str}' ({skipped} records skipped)",
        n_users = by_user.len(),
        n_records = by_user.values().map(|v| v.len()).sum::<usize>(),
    );

    if by_user.is_empty() {
        eprintln!("nothing to migrate, we're done here ✨");
        return Ok(());
    }

    let mut stats = MigrationStats {
        users_created: 0,
        users_deleted: 0,
        records_imported: 0,
        records_already_synced: 0,
        records_deleted: 0,
    };

    let upstream_usernames: HashSet<&str> = by_user.keys().map(|s| s.as_str()).collect();

    for (username, upstream) in &by_user {
        eprintln!("\n--- {username} ({} records upstream) ---", upstream.len());

        let user_id = ensure_user_exists(user_db, username, &mut stats, dry_run).await?;

        sync_records_for_user(user_db, user_id, username, upstream, &mut stats, dry_run).await?;
    }

    // Check for users in the DB that have no records upstream
    cleanup_orphaned_users(user_db, &upstream_usernames, &mut stats, dry_run).await?;

    if dry_run {
        eprintln!("\n(dry run -- nothing was actually changed)");
    }

    let label = if dry_run { "dry run" } else { "migration" };
    eprintln!("\n========== {label} summary ==========");
    if dry_run {
        eprintln!("  users to create:         {}", stats.users_created);
        eprintln!("  users to delete:         {}", stats.users_deleted);
        eprintln!("  records to import:       {}", stats.records_imported);
        eprintln!("  records to delete:       {}", stats.records_deleted);
    } else {
        eprintln!("  users created:           {}", stats.users_created);
        eprintln!("  users deleted:           {}", stats.users_deleted);
        eprintln!("  records imported:        {}", stats.records_imported);
        eprintln!("  records deleted:         {}", stats.records_deleted);
    }
    eprintln!(
        "  records already synced:  {}",
        stats.records_already_synced
    );
    eprintln!("========================================");

    Ok(())
}

/// Make sure a user exists in the database. If they don't, interactively prompt
/// for password + email and create + activate them.
/// Returns `Some(uuid)` on success, or `None` in dry-run mode when the user
/// doesn't exist yet (we can't get an ID without actually creating them).
async fn ensure_user_exists(
    user_db: &UserDatabase,
    username: &str,
    stats: &mut MigrationStats,
    dry_run: bool,
) -> Result<Option<fckn_gay_user_database::Uuid>> {
    if user_db.is_available(username).await {
        if dry_run {
            eprintln!(
                "  🆕 would create user '{username}' (password: random, email: {DEFAULT_EMAIL} unless overridden)"
            );
            stats.users_created += 1;
            return Ok(None);
        }

        // User doesn't exist yet -- create them
        let password = match prompt("Password [blank = random]: ") {
            Some(pw) => {
                eprintln!("  (using provided password)");
                pw
            }
            None => {
                let pw = generate_random_password();
                eprintln!("  password generated!");
                pw
            }
        };

        let email = prompt(&format!("Email [blank = {DEFAULT_EMAIL}]: "))
            .unwrap_or_else(|| DEFAULT_EMAIL.to_string());

        let uuid = user_db
            .add_user(username, &password, &email)
            .await
            .map_err(|e| anyhow::anyhow!("failed to create user '{username}': {e}"))?;

        // Skip the email confirmation dance -- this is an admin migration
        user_db
            .activate_user(uuid)
            .await
            .map_err(|e| anyhow::anyhow!("failed to activate user '{username}': {e}"))?;

        eprintln!("  ✅ created + activated user '{username}'");
        stats.users_created += 1;
        Ok(Some(uuid))
    } else {
        // User exists -- grab their ID
        let entry = user_db
            .get_user_by_username_or_email(username)
            .await
            .map_err(|e| anyhow::anyhow!("failed to look up user '{username}': {e}"))?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "user '{username}' is marked as unavailable but we can't find them 💀"
                )
            })?;

        eprintln!("  user '{username}' already exists (id: {})", entry.id);
        Ok(Some(entry.id))
    }
}

/// Compare upstream DNS records with what's in the user database for a given user.
/// Import anything missing from the DB, report anything in the DB but gone upstream.
///
/// `user_id` is `None` when dry-running for a user that doesn't exist yet --
/// in that case every upstream record counts as "would be imported".
async fn sync_records_for_user(
    user_db: &UserDatabase,
    user_id: Option<fckn_gay_user_database::Uuid>,
    username: &str,
    upstream: &[(String, fckn_gay_dns::Record)],
    stats: &mut MigrationStats,
    dry_run: bool,
) -> Result<()> {
    // If we don't have a user_id (dry-run for a new user), there are no DB
    // records to compare against -- everything upstream would be imported.
    let db_records = match user_id {
        Some(id) => user_db
            .get_user_dns_records(id)
            .await
            .map_err(|e| anyhow::anyhow!("failed to fetch DB records for '{username}': {e}"))?,
        None => Vec::new(),
    };

    // Set of provider keys that exist upstream
    let upstream_keys: HashSet<&str> = upstream.iter().map(|(k, _)| k.as_str()).collect();

    // Set of provider keys already in the DB
    let db_keys: HashSet<&str> = db_records.iter().map(|r| r.provider_key.as_str()).collect();

    let verb = if dry_run { "would import" } else { "imported" };
    let icon = if dry_run { "📋" } else { "📥" };

    // Records upstream but not in DB -> import (or report)
    for (provider_key, record) in upstream {
        if db_keys.contains(provider_key.as_str()) {
            stats.records_already_synced += 1;
            continue;
        }

        if !dry_run && let Some(id) = user_id {
            user_db
                .add_dns_record(id, record.clone(), provider_key.clone())
                .await
                .map_err(|e| {
                    anyhow::anyhow!(
                        "failed to import record '{}' ({}) for '{username}': {e}",
                        record.name,
                        record.record_type,
                    )
                })?;
        }

        eprintln!(
            "  {icon} {verb}: {} {} -> {}",
            record.name, record.record_type, record.content
        );
        stats.records_imported += 1;
    }

    // Records in DB but not upstream -> delete locally
    for db_rec in &db_records {
        if upstream_keys.contains(db_rec.provider_key.as_str()) {
            continue;
        }

        if dry_run {
            eprintln!(
                "  🗑️  would delete local record (gone upstream): {} {} -> {}",
                db_rec.record.name, db_rec.record.record_type, db_rec.record.content,
            );
        } else if let Some(id) = user_id {
            user_db
                .delete_dns_record(id, db_rec.id)
                .await
                .map_err(|e| {
                    anyhow::anyhow!(
                        "failed to delete stale record '{}' ({}) for '{username}': {e}",
                        db_rec.record.name,
                        db_rec.record.record_type,
                    )
                })?;
            eprintln!(
                "  🗑️  deleted local record (gone upstream): {} {} -> {}",
                db_rec.record.name, db_rec.record.record_type, db_rec.record.content,
            );
        }
        stats.records_deleted += 1;
    }

    Ok(())
}

/// Find users in the local DB that don't have any records upstream and offer to
/// delete them (including their local DNS records).
async fn cleanup_orphaned_users(
    user_db: &UserDatabase,
    upstream_usernames: &HashSet<&str>,
    stats: &mut MigrationStats,
    dry_run: bool,
) -> Result<()> {
    let all_users = user_db
        .list_all_users()
        .await
        .map_err(|e| anyhow::anyhow!("failed to list all users: {e}"))?;

    let orphans: Vec<_> = all_users
        .iter()
        .filter(|u| !upstream_usernames.contains(u.username.as_str()))
        .collect();

    if orphans.is_empty() {
        return Ok(());
    }

    eprintln!(
        "\n🔎 found {} user(s) in DB with no records upstream:",
        orphans.len()
    );
    for user in &orphans {
        let n_records = user_db
            .get_user_dns_records(user.id)
            .await
            .map(|r| r.len())
            .unwrap_or(0);

        if dry_run {
            eprintln!(
                "  🗑️  would delete user '{}' ({n_records} local records)",
                user.username
            );
            stats.users_deleted += 1;
            stats.records_deleted += n_records as u32;
            continue;
        }

        eprint!(
            "  Delete user '{}' ({n_records} local records)? [y/N] ",
            user.username
        );
        io::stderr().flush().ok();
        let mut answer = String::new();
        io::stdin()
            .lock()
            .read_line(&mut answer)
            .expect("failed to read stdin");

        if answer.trim().eq_ignore_ascii_case("y") {
            // Delete their DNS records first, then the user
            let records = user_db.get_user_dns_records(user.id).await.map_err(|e| {
                anyhow::anyhow!(
                    "failed to fetch records for orphaned user '{}': {e}",
                    user.username
                )
            })?;
            for rec in &records {
                user_db
                    .delete_dns_record(user.id, rec.id)
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!("failed to delete record for '{}': {e}", user.username)
                    })?;
            }
            user_db
                .delete_user(&user.username)
                .await
                .map_err(|e| anyhow::anyhow!("failed to delete user '{}': {e}", user.username))?;
            eprintln!(
                "  🗑️  deleted user '{}' + {n_records} records",
                user.username
            );
            stats.users_deleted += 1;
            stats.records_deleted += records.len() as u32;
        } else {
            eprintln!("  ⏭️  skipped '{}'", user.username);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_username_basic() {
        assert_eq!(
            extract_username("admin.is.fckn.gay", ".is.fckn.gay"),
            Some("admin".into())
        );
    }

    #[test]
    fn extract_username_nested_subdomain() {
        assert_eq!(
            extract_username("sub.alice.is.fckn.gay", ".is.fckn.gay"),
            Some("alice".into())
        );
    }

    #[test]
    fn extract_username_case_insensitive() {
        assert_eq!(
            extract_username("ALICE.Is.Fckn.Gay", ".is.fckn.gay"),
            Some("alice".into())
        );
    }

    #[test]
    fn extract_username_wrong_suffix() {
        assert_eq!(extract_username("admin.other.domain", ".is.fckn.gay"), None);
    }

    #[test]
    fn extract_username_bare_suffix() {
        assert_eq!(extract_username(".is.fckn.gay", ".is.fckn.gay"), None);
    }

    #[test]
    fn extract_username_wildcard() {
        assert_eq!(extract_username("*.is.fckn.gay", ".is.fckn.gay"), None);
    }

    #[test]
    fn extract_username_nested_wildcard() {
        // *.alice.is.fckn.gay -> username is alice, not *
        assert_eq!(
            extract_username("*.alice.is.fckn.gay", ".is.fckn.gay"),
            Some("alice".into())
        );
    }

    #[test]
    fn random_password_is_32_chars() {
        let pw = generate_random_password();
        assert_eq!(pw.len(), 32);
        assert!(pw.is_ascii());
    }
}
