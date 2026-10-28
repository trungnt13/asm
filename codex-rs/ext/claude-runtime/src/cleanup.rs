pub(super) type ProcessBirth = (u64, u64);

pub(super) const OWNER_ENV: &str = "ASM_NATIVE_PROCESS_OWNER";
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) use supported::kill_owned;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) use supported::process_birth;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn kill_owned(
    _pid: Option<u32>,
    _marker: &str,
    _birth: Option<ProcessBirth>,
) -> anyhow::Result<()> {
    anyhow::bail!("native process cleanup is unsupported")
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn process_birth(_pid: u32) -> anyhow::Result<ProcessBirth> {
    anyhow::bail!("native process identity is unsupported")
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod supported {
    use super::OWNER_ENV;
    use super::ProcessBirth;
    use anyhow::Context;
    use anyhow::Result;
    use anyhow::bail;
    use std::collections::HashSet;
    use std::time::Duration;

    const PROCESS_LIST_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 2);
    // The marker follows ordinary native tools across setsid and leader exit. It is not a sandbox:
    // a process that clears its environment and leaves the observed ancestry can escape ownership.
    pub(crate) fn kill_owned(
        pid: Option<u32>,
        marker: &str,
        birth: Option<ProcessBirth>,
    ) -> Result<()> {
        let deadline = std::time::Instant::now() + PROCESS_LIST_TIMEOUT;
        let uid = current_uid();
        let mut bytes = Vec::new();
        let cleanup = (|| -> Result<()> {
            loop {
                let table = list_processes(deadline)?;
                let own_group = table
                    .iter()
                    .find(|row| row.pid == std::process::id())
                    .context("ASM is missing from the process list")?
                    .group;
                let mut tree = HashSet::new();
                if let Some(pid) = pid
                    && owns_marker(pid, marker, &mut bytes).unwrap_or(false)
                    && birth
                        .is_some_and(|birth| process_birth(pid).is_ok_and(|actual| actual == birth))
                {
                    tree.insert(pid);
                }
                let mut changed = true;
                while changed {
                    changed = false;
                    for row in &table {
                        if tree.contains(&row.parent) && tree.insert(row.pid) {
                            changed = true;
                        }
                    }
                }
                let mut found = false;
                let mut scan_error = None;
                for row in &table {
                    if row.zombie || row.group == own_group || row.uid != uid {
                        continue;
                    }
                    if std::time::Instant::now() >= deadline {
                        scan_error.get_or_insert_with(|| {
                            anyhow::anyhow!("native ownership scan timed out")
                        });
                        break;
                    }
                    // Older processes cannot descend from this runtime, including inaccessible host daemons.
                    if let Some(birth) = birth
                        && row.birth.is_some_and(|actual| actual < birth)
                    {
                        continue;
                    }
                    let (marked, ownership_error) = match owns_marker(row.pid, marker, &mut bytes) {
                        Ok(marked) => (marked, None),
                        Err(error) => (false, Some(error)),
                    };
                    if !marked && !tree.contains(&row.pid) {
                        if let Some(error) = ownership_error {
                            scan_error.get_or_insert(error);
                        }
                        continue;
                    }
                    // Recheck marked ownership just before signalling, rather than trust a recycled PID.
                    if marked {
                        match owns_marker(row.pid, marker, &mut bytes) {
                            Ok(true) => {}
                            Ok(false) => continue,
                            Err(error) => {
                                scan_error.get_or_insert(error);
                                continue;
                            }
                        }
                    }
                    if !marked
                        && !row.birth.is_some_and(|birth| {
                            process_birth(row.pid).is_ok_and(|actual| actual == birth)
                        })
                    {
                        scan_error.get_or_insert_with(|| {
                            anyhow::anyhow!("native descendant identity changed or is unavailable")
                        });
                        continue;
                    }
                    found = true;
                    // SAFETY: the PID belongs to this native runtime, not ASM's group.
                    let result = unsafe { libc::kill(row.pid as i32, libc::SIGKILL) };
                    if result != 0 {
                        let error = std::io::Error::last_os_error();
                        if error.raw_os_error() != Some(libc::ESRCH) {
                            scan_error.get_or_insert_with(|| {
                                anyhow::anyhow!(error).context("killing native process")
                            });
                        }
                    }
                }
                if let Some(error) = scan_error {
                    return Err(error);
                }
                if !found {
                    return Ok(());
                }
                if std::time::Instant::now() >= deadline {
                    bail!("native processes survived cleanup deadline");
                }
                std::thread::sleep(Duration::from_millis(/*millis*/ 20));
            }
        })();
        if let Some(pid) = pid
            && owns_marker(pid, marker, &mut bytes).unwrap_or(false)
            && birth.is_some_and(|birth| process_birth(pid).is_ok_and(|actual| actual == birth))
        {
            let group = codex_utils_pty::process_group::kill_process_group(pid);
            if cleanup.is_ok() {
                group.context("killing native process group")?;
            }
        }
        cleanup
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn process_birth(pid: u32) -> Result<ProcessBirth> {
        let stat = std::fs::read(format!("/proc/{pid}/stat"))?;
        let end = stat
            .iter()
            .rposition(|byte| *byte == b')')
            .context("invalid process stat")?;
        let ticks = std::str::from_utf8(&stat[end + 1..])?
            .split_whitespace()
            .nth(/*n*/ 19)
            .context("missing process birth")?
            .parse()?;
        Ok((ticks, 0))
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn process_birth(pid: u32) -> Result<ProcessBirth> {
        let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        // SAFETY: the output buffer has the exact size and alignment required by this flavor.
        let read = unsafe {
            libc::proc_pidinfo(
                pid as i32,
                libc::PROC_PIDTBSDINFO,
                /*arg*/ 1,
                info.as_mut_ptr().cast(),
                size,
            )
        };
        anyhow::ensure!(
            read == size,
            "cannot read process birth: {}",
            std::io::Error::last_os_error()
        );
        // SAFETY: proc_pidinfo initialized the complete structure on success.
        let info = unsafe { info.assume_init() };
        Ok((info.pbi_start_tvsec, info.pbi_start_tvusec))
    }

    fn current_uid() -> u32 {
        // SAFETY: getuid has no arguments or failure case.
        unsafe { libc::getuid() }
    }

    fn owns_marker(pid: u32, marker: &str, bytes: &mut Vec<u8>) -> Result<bool> {
        let expected = format!("{OWNER_ENV}={marker}");
        match environment_bytes(pid, bytes) {
            Ok(()) => Ok(bytes
                .split(|byte| *byte == 0)
                .any(|value| value == expected.as_bytes())),
            Err(error) if matches!(error.raw_os_error(), Some(libc::ESRCH | libc::ENOENT)) => {
                Ok(false)
            }
            Err(error) => Err(error).context("checking native process ownership"),
        }
    }

    #[cfg(target_os = "linux")]
    fn environment_bytes(pid: u32, bytes: &mut Vec<u8>) -> std::io::Result<()> {
        use std::io::Read;
        bytes.clear();
        std::fs::File::open(format!("/proc/{pid}/environ"))?
            .take(/*limit*/ 4 * 1024 * 1024 + 1)
            .read_to_end(bytes)?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(std::io::Error::other(
                "native ownership environment exceeds scan limit",
            ));
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn environment_bytes(pid: u32, bytes: &mut Vec<u8>) -> std::io::Result<()> {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as i32];
        bytes.resize(4 * 1024 * 1024, 0);
        let mut length = bytes.len();
        // SAFETY: sysctl receives a valid MIB and a writable buffer of the supplied length.
        let result = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as u32,
                bytes.as_mut_ptr().cast(),
                &mut length,
                std::ptr::null_mut(),
                /*newlen*/ 0,
            )
        };
        if result != 0 {
            return Err(std::io::Error::last_os_error());
        }
        bytes.truncate(length);
        // KERN_PROCARGS2 begins with argc, executable path, padding, then argv and environment.
        // Skip argv so a matching argument cannot claim process ownership.
        let argc = bytes
            .get(..4)
            .and_then(|bytes| bytes.try_into().ok())
            .map(i32::from_ne_bytes)
            .filter(|argc| *argc >= 0)
            .ok_or_else(|| std::io::Error::other("invalid native process arguments"))?;
        let mut offset = 4;
        offset += bytes[offset..]
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| std::io::Error::other("missing native executable terminator"))?
            + 1;
        while bytes.get(offset) == Some(&0) {
            offset += 1;
        }
        for _ in 0..argc {
            offset += bytes
                .get(offset..)
                .and_then(|bytes| bytes.iter().position(|byte| *byte == 0))
                .ok_or_else(|| {
                    std::io::Error::other("invalid native process argument terminator")
                })?
                + 1;
        }
        bytes.drain(..offset);
        Ok(())
    }

    struct ProcessRow {
        pid: u32,
        parent: u32,
        group: u32,
        uid: u32,
        zombie: bool,
        birth: Option<ProcessBirth>,
    }

    fn list_processes(deadline: std::time::Instant) -> Result<Vec<ProcessRow>> {
        use std::io::Read;
        use std::io::Seek;
        let mut output = tempfile::tempfile()?;
        let mut child = std::process::Command::new("ps")
            .args(["-A", "-o", "pid=,ppid=,pgid=,uid=,stat="])
            .stdin(std::process::Stdio::null())
            .stdout(output.try_clone()?)
            .stderr(std::process::Stdio::null())
            .spawn()?;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                bail!("ps timed out");
            }
            std::thread::sleep(Duration::from_millis(/*millis*/ 10));
        };
        anyhow::ensure!(status.success(), "ps failed: {status}");
        let mut text = String::new();
        output.rewind()?;
        output.read_to_string(&mut text)?;
        text.lines()
            .map(|line| {
                anyhow::ensure!(
                    std::time::Instant::now() < deadline,
                    "process metadata scan timed out"
                );
                let mut fields = line.split_whitespace();
                let mut number = || {
                    fields
                        .next()
                        .context("missing process field")?
                        .parse::<u32>()
                        .context("invalid process field")
                };
                let (pid, parent, group, uid) = (number()?, number()?, number()?, number()?);
                let zombie = fields
                    .next()
                    .context("missing process state")?
                    .starts_with('Z');
                Ok(ProcessRow {
                    pid,
                    parent,
                    group,
                    uid,
                    zombie,
                    birth: (uid == current_uid())
                        .then(|| process_birth(pid).ok())
                        .flatten(),
                })
            })
            .collect()
    }
}
