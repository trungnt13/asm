use codex_protocol::ThreadId;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;

pub(crate) fn ensure_no_runtime_owner(
    codex_home: &Path,
    thread_ids: &[ThreadId],
) -> io::Result<()> {
    lock_runtime_data(codex_home, thread_ids).map(drop)
}

// Runs after rollout removal, while thread-store writer ownership excludes native startup.
pub(crate) fn delete_thread_data(codex_home: &Path, thread_ids: &[ThreadId]) -> io::Result<()> {
    let locks = lock_runtime_data(codex_home, thread_ids)?;
    let directory = codex_home.join("claude-runtime");
    let owners: HashSet<String> = locks
        .iter()
        .map(|(thread_id, _)| thread_id.to_string())
        .collect();
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    // Every `<thread>.*` entry is private to that thread: linkage, Cursor configuration,
    // diagnostics, login markers, and interrupted checkpoint writes.
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some((owner, suffix)) = name.to_str().and_then(|name| name.split_once('.')) else {
            continue;
        };
        if !owners.contains(owner) || suffix == "lock" {
            continue;
        }
        let result = if entry.file_type()?.is_dir() {
            fs::remove_dir_all(entry.path())
        } else {
            fs::remove_file(entry.path())
        };
        if let Err(err) = result
            && err.kind() != io::ErrorKind::NotFound
        {
            return Err(err);
        }
    }
    for (thread_id, lock) in locks {
        // Close first for Windows; the outer writer lock still excludes native startup.
        drop(lock);
        if let Err(err) = fs::remove_file(directory.join(format!("{thread_id}.lock")))
            && err.kind() != io::ErrorKind::NotFound
        {
            return Err(err);
        }
    }
    Ok(())
}

fn lock_runtime_data(
    codex_home: &Path,
    thread_ids: &[ThreadId],
) -> io::Result<Vec<(ThreadId, Option<fs::File>)>> {
    let directory = codex_home.join("claude-runtime");
    let mut locks = Vec::with_capacity(thread_ids.len());
    let mut seen = HashSet::new();
    for thread_id in thread_ids {
        if !seen.insert(*thread_id) {
            continue;
        }
        let path = directory.join(format!("{thread_id}.lock"));
        match fs::OpenOptions::new().write(true).open(path) {
            Ok(file) => {
                match file.try_lock() {
                    Ok(()) => {}
                    Err(fs::TryLockError::WouldBlock) => {
                        return Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            format!("native runtime for thread {thread_id} is still owned"),
                        ));
                    }
                    Err(fs::TryLockError::Error(err)) => return Err(err),
                }
                locks.push((*thread_id, Some(file)));
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => locks.push((*thread_id, None)),
            Err(err) => return Err(err),
        }
    }
    Ok(locks)
}
