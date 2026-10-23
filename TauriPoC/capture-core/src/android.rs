use crate::{model::*, process};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap, path::Path, process::Command, sync::atomic::AtomicBool, time::Duration,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AndroidApplication {
    pub application_id: String,
    pub label: String,
    pub user_id: u32,
    pub uid: u32,
    pub shared_uid: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogStrategy {
    WholeDevice,
    AndroidUid,
    AndroidProcessMap,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogAttribution {
    pub strategy: LogStrategy,
    pub application_id: Option<String>,
    pub user_id: Option<u32>,
    pub uid: Option<u32>,
    pub install_identity: Option<String>,
    pub levels: String,
    pub buffers: Vec<String>,
    pub exclusions: Vec<String>,
}
impl Default for LogAttribution {
    fn default() -> Self {
        Self {
            strategy: LogStrategy::WholeDevice,
            application_id: None,
            user_id: None,
            uid: None,
            install_identity: None,
            levels: "all_available_emitted_levels".into(),
            buffers: vec!["main".into(), "system".into(), "crash".into()],
            exclusions: vec!["OS-redacted or build-disabled logging cannot be recovered".into()],
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResolvedLogs {
    pub attribution: LogAttribution,
}
#[derive(Clone, Debug, PartialEq)]
struct ProcessIdentity {
    start_ticks: u64,
    started_before_epoch: f64,
}
#[derive(Clone, Debug)]
pub(crate) struct IdentitySnapshot {
    fence: f64,
    processes: BTreeMap<u32, ProcessIdentity>,
}

pub(crate) fn valid_application_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value.bytes().enumerate().all(|(i, b)| {
            b.is_ascii_alphanumeric() || b == b'_' || (i > 0 && matches!(b, b'.' | b'-'))
        })
}
fn validate_serial(value: &str) -> Result<()> {
    if value.is_empty() || value.starts_with('-') || value.chars().any(char::is_whitespace) {
        return Err("Invalid Android device identifier".into());
    }
    Ok(())
}
pub(crate) fn execute(adb: &Path, serial: &str, arguments: &[&str]) -> Result<(String, String)> {
    execute_with_cancel(adb, serial, arguments, None)
}
fn execute_with_cancel(
    adb: &Path,
    serial: &str,
    arguments: &[&str],
    cancelled: Option<&AtomicBool>,
) -> Result<(String, String)> {
    validate_serial(serial)?;
    let mut command = Command::new(adb);
    process::quiet(&mut command);
    command
        .args(["-H", "127.0.0.1", "-P", "5037", "-s", serial])
        .args(arguments);
    let output = process::output_bounded(
        &mut command,
        Duration::from_secs(8),
        cancelled,
        2 * 1024 * 1024,
        2 * 1024 * 1024,
    )
    .map_err(|error| match error {
        process::OutputError::Cancelled => {
            "Android identity probe cancelled by source lease closure".into()
        }
        process::OutputError::TimedOut => {
            "Android identity probe exceeded its eight second deadline".into()
        }
        process::OutputError::Limit { .. } => "Android identity metadata exceeded 2 MiB".into(),
        process::OutputError::Io(error) => format!("Android identity probe failed: {error}"),
    })?;
    let stdout =
        String::from_utf8(output.stdout).map_err(|_| "Android identity metadata was not UTF-8")?;
    let stderr =
        String::from_utf8(output.stderr).map_err(|_| "Android identity metadata was not UTF-8")?;
    if !output.status.success() {
        return Err(format!(
            "Android identity probe failed. {}",
            stderr.chars().take(1200).collect::<String>()
        ));
    }
    Ok((stdout, stderr))
}

fn packages(output: &str, user: u32) -> Result<BTreeMap<String, u32>> {
    let mut result = BTreeMap::new();
    for line in output.lines().filter(|line| line.starts_with("package:")) {
        let fields: Vec<_> = line.split_whitespace().collect();
        let id = fields[0].trim_start_matches("package:");
        let uid: u32 = fields
            .iter()
            .find_map(|f| f.strip_prefix("uid:"))
            .ok_or("Package UID missing")?
            .parse()
            .map_err(|_| "Invalid package UID")?;
        if !valid_application_id(id) || uid / 100_000 != user {
            return Err("Android package UID/user identity could not be verified".into());
        }
        if result
            .insert(id.to_owned(), uid)
            .is_some_and(|prior| prior != uid)
        {
            return Err("Conflicting Android package UID identities".into());
        }
    }
    Ok(result)
}
fn install_identity(output: &str) -> Result<String> {
    let fields: Vec<_> = output.lines().map(str::trim).collect();
    let mut identity = Vec::new();
    for prefix in ["codePath=", "firstInstallTime=", "lastUpdateTime="] {
        let values: Vec<_> = fields.iter().filter(|f| f.starts_with(prefix)).collect();
        if values.len() != 1 || values[0].len() == prefix.len() {
            return Err("Android installation identity is unavailable or ambiguous".into());
        }
        identity.push(*values[0]);
    }
    Ok(format!(
        "{:x}",
        Sha256::digest(identity.join("\n").as_bytes())
    ))
}
pub fn applications(adb: &Path, serial: &str) -> Result<Vec<AndroidApplication>> {
    let (output, _) = execute(adb, serial, &["shell", "am", "get-current-user"])?;
    let user: u32 = output
        .trim()
        .parse()
        .map_err(|_| "Active Android user could not be verified")?;
    let (output, _) = execute(
        adb,
        serial,
        &[
            "shell",
            "pm",
            "list",
            "packages",
            "--user",
            &user.to_string(),
            "-U",
        ],
    )?;
    let values = packages(&output, user)?;
    Ok(values
        .iter()
        .map(|(id, uid)| AndroidApplication {
            application_id: id.clone(),
            label: id.clone(),
            user_id: user,
            uid: *uid,
            shared_uid: values.values().filter(|other| *other == uid).count() != 1,
        })
        .collect())
}
pub(crate) fn resolve(adb: &Path, serial: &str, scope: &LogScope) -> Result<ResolvedLogs> {
    let LogScope::App { application_id } = scope else {
        return Ok(ResolvedLogs {
            attribution: LogAttribution::default(),
        });
    };
    if !valid_application_id(application_id) {
        return Err("Choose a valid installed Android application".into());
    }
    let app = applications(adb, serial)?
        .into_iter()
        .find(|a| &a.application_id == application_id)
        .ok_or("Selected app is no longer installed for the active Android user")?;
    if app.shared_uid {
        return Err("Selected app shares its UID. Choose explicit whole-device logs or network-only capture".into());
    }
    let (package, _) = execute(
        adb,
        serial,
        &["shell", "dumpsys", "package", application_id],
    )?;
    let identity = install_identity(&package)?;
    let (help, errors) = execute(adb, serial, &["shell", "logcat", "--help"])?;
    let uid_argument = format!("--uid={}", app.uid);
    let uid_capable = (help + &errors).contains("--uid")
        && execute(
            adb,
            serial,
            &["logcat", &uid_argument, "-d", "-t", "1", "-v", "epoch"],
        )
        .is_ok();
    let resolved = ResolvedLogs {
        attribution: LogAttribution {
            strategy: if uid_capable {
                LogStrategy::AndroidUid
            } else {
                LogStrategy::AndroidProcessMap
            },
            application_id: Some(application_id.clone()),
            user_id: Some(app.user_id),
            uid: Some(app.uid),
            install_identity: Some(identity),
            exclusions: vec![
                "Other UIDs, isolated SDK processes, and helpers under other UIDs are excluded"
                    .into(),
                "Identity loss or ambiguous PID reuse stops logs with a coverage gap".into(),
                "Records before the first verified device-time fence are excluded".into(),
                "PID fallback may omit processes born and exited entirely between probes".into(),
            ],
            ..LogAttribution::default()
        },
    };
    resolved.snapshot(adb, serial)?;
    Ok(resolved)
}
fn section<'a>(output: &'a str, first: &str, last: &str) -> Result<&'a str> {
    let start = output
        .find(&format!("{first}\n"))
        .ok_or("Android identity metadata is incomplete")?
        + first.len()
        + 1;
    let end = output[start..]
        .find(last)
        .ok_or("Android identity metadata is incomplete")?
        + start;
    Ok(output[start..end].trim())
}
impl ResolvedLogs {
    pub(crate) fn snapshot(&self, adb: &Path, serial: &str) -> Result<IdentitySnapshot> {
        self.snapshot_with_cancel(adb, serial, None)
    }
    pub(crate) fn snapshot_with_cancel(
        &self,
        adb: &Path,
        serial: &str,
        cancelled: Option<&AtomicBool>,
    ) -> Result<IdentitySnapshot> {
        let a = &self.attribution;
        let id = a.application_id.as_deref().ok_or("App identity missing")?;
        if !valid_application_id(id) {
            return Err("Invalid app identity configuration".into());
        }
        let uid = a.uid.ok_or("App UID missing")?;
        let user = a.user_id.ok_or("Android user missing")?;
        let details = if a.strategy == LogStrategy::AndroidProcessMap {
            format!("echo __MC_CLOCK__\ndate +%s.%N\ncat /proc/uptime\ndate +%s.%N\ngetconf CLK_TCK\necho __MC_PS__\nps -A -o PID,UID,NAME\necho __MC_DETAILS__\nfor p in $(ps -A -o PID,UID,NAME | awk '$2 == {uid} {{print $1}}'); do\necho __MC_PID_$p\ncat /proc/$p/stat 2>/dev/null || echo __MC_MISSING__\nsed -n '/^Uid:/p' /proc/$p/status 2>/dev/null || true\ntr '\\000' '\\n' < /proc/$p/cmdline 2>/dev/null | head -n 1 || true\necho\ndone\necho __MC_END__")
        } else {
            "echo __MC_END__".into()
        };
        let script = format!("set -e\necho __MC_FENCE__\ndate +%s.%N\necho __MC_USER__\nam get-current-user\necho __MC_PACKAGES__\npm list packages --user {user} -U\necho __MC_PACKAGE__\ndumpsys package '{id}'\n{details}");
        let (output, _) = execute_with_cancel(adb, serial, &["shell", &script], cancelled)?;
        self.parse_snapshot(&output)
    }
    fn parse_snapshot(&self, output: &str) -> Result<IdentitySnapshot> {
        let a = &self.attribution;
        let user: u32 = section(output, "__MC_USER__", "__MC_PACKAGES__")?
            .parse()
            .map_err(|_| "Android user metadata invalid")?;
        if Some(user) != a.user_id {
            return Err("Active Android user changed. Logs stopped with an identity gap".into());
        }
        let fence: f64 = section(output, "__MC_FENCE__", "__MC_USER__")?
            .parse()
            .map_err(|_| "Android clock fence invalid")?;
        if !fence.is_finite() || fence <= 0.0 {
            return Err("Android clock cannot fence identity checks".into());
        }
        let values = packages(section(output, "__MC_PACKAGES__", "__MC_PACKAGE__")?, user)?;
        let uid = a.uid.ok_or("App UID missing")?;
        if values.get(a.application_id.as_deref().ok_or("App missing")?) != Some(&uid)
            || values.values().filter(|v| **v == uid).count() != 1
        {
            return Err(
                "Package UID changed or became shared. Logs stopped with an identity gap".into(),
            );
        }
        let end = if a.strategy == LogStrategy::AndroidProcessMap {
            "__MC_CLOCK__"
        } else {
            "__MC_END__"
        };
        if Some(install_identity(section(output, "__MC_PACKAGE__", end)?)?) != a.install_identity {
            return Err("App reinstalled or updated. Logs stopped with an identity gap".into());
        }
        let mut processes = BTreeMap::new();
        if a.strategy == LogStrategy::AndroidUid {
            return Ok(IdentitySnapshot { fence, processes });
        }
        let clocks: Vec<_> = section(output, "__MC_CLOCK__", "__MC_PS__")?
            .lines()
            .collect();
        if clocks.len() != 4 {
            return Err("Android process lifetime clock metadata unavailable".into());
        }
        let parse = |s: &str| s.parse::<f64>().ok().filter(|v| v.is_finite());
        let before = parse(clocks[0]).ok_or("Invalid process clock")?;
        let uptime =
            parse(clocks[1].split_whitespace().next().unwrap_or("")).ok_or("Invalid uptime")?;
        let after = parse(clocks[2]).ok_or("Invalid process clock")?;
        let ticks = parse(clocks[3]).ok_or("Invalid process clock ticks")?;
        if before < fence || after < before || uptime < 0.0 || ticks <= 0.0 {
            return Err("Android process lifetime clock metadata inconsistent".into());
        }
        let table = section(output, "__MC_PS__", "__MC_DETAILS__")?;
        if table
            .lines()
            .next()
            .map(|l| l.split_whitespace().collect::<Vec<_>>())
            != Some(vec!["PID", "UID", "NAME"])
        {
            return Err("Android process table unavailable".into());
        }
        let mut candidates = BTreeMap::new();
        for line in table.lines().skip(1) {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() >= 3 && fields[1].parse::<u32>() == Ok(uid) {
                let pid: u32 = fields[0].parse().map_err(|_| "Invalid process PID")?;
                if candidates.insert(pid, fields[2..].join(" ")).is_some() {
                    return Err("Duplicate process identity".into());
                }
            }
        }
        for block in section(output, "__MC_DETAILS__", "__MC_END__")?
            .split("__MC_PID_")
            .skip(1)
        {
            let lines: Vec<_> = block.lines().collect();
            if lines.len() < 4 {
                return Err("Android app process lifetime metadata unavailable".into());
            }
            let pid: u32 = lines[0].parse().map_err(|_| "Invalid detail PID")?;
            let Some(name) = candidates.get(&pid) else {
                continue;
            };
            if lines[1]
                .split_whitespace()
                .next()
                .and_then(|p| p.parse::<u32>().ok())
                != Some(pid)
            {
                return Err("Android process stat identity changed during verification".into());
            }
            let closing = lines[1]
                .rfind(')')
                .ok_or("Android process changed during verification")?;
            let fields: Vec<_> = lines[1][closing + 1..].split_whitespace().collect();
            let ids: Vec<_> = lines[2].split_whitespace().collect();
            let start_ticks = fields
                .get(19)
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or("Android process start identity unavailable")?;
            if ids.len() < 5
                || ids[0] != "Uid:"
                || ids[1..5].iter().any(|s| s.parse::<u32>() != Ok(uid))
                || lines[3] != name
            {
                return Err("Android process identity could not be maintained".into());
            }
            if processes
                .insert(
                    pid,
                    ProcessIdentity {
                        start_ticks,
                        started_before_epoch: after - uptime + (start_ticks as f64 + 1.0) / ticks,
                    },
                )
                .is_some()
            {
                return Err("Duplicate Android process metadata".into());
            }
        }
        if processes.len() != candidates.len() {
            return Err(
                "Android app process metadata is unreadable. Safe PID fallback unavailable".into(),
            );
        }
        Ok(IdentitySnapshot {
            fence: before,
            processes,
        })
    }
}

pub(crate) struct LogFilter {
    strategy: LogStrategy,
    pending: Vec<u8>,
    previous: Option<IdentitySnapshot>,
    earliest: Option<f64>,
    continuation: bool,
    header: Regex,
    malformed: Regex,
}
impl LogFilter {
    pub(crate) fn new(resolved: &ResolvedLogs) -> Self {
        Self {
            strategy: resolved.attribution.strategy.clone(),
            pending: Vec::new(),
            previous: None,
            earliest: None,
            continuation: false,
            header: Regex::new(r"^\s*(\d{9,}(?:\.\d+)?)\s+(\d+)\s+\d+\s+[VDIWEFAS]\s+").unwrap(),
            malformed: Regex::new(r"^\s*\d{9,}(?:\.\d+)?\s+").unwrap(),
        }
    }
    pub(crate) fn append(&mut self, data: &[u8]) -> Result<()> {
        if self.pending.len().saturating_add(data.len()) > 2 * 1024 * 1024 {
            return Err("Identity verification could not keep up. Logs stopped with a coverage gap; unverified backlog was discarded".into());
        }
        self.pending.extend_from_slice(data);
        Ok(())
    }
    pub(crate) fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
    pub(crate) fn verify(&mut self, snapshot: IdentitySnapshot) -> Result<Vec<u8>> {
        if !snapshot.fence.is_finite()
            || snapshot.fence <= 0.0
            || self
                .previous
                .as_ref()
                .is_some_and(|p| snapshot.fence < p.fence)
        {
            return Err("Android clock rolled back. Logs stopped with a coverage gap".into());
        }
        let Some(previous) = &self.previous else {
            self.earliest = Some(snapshot.fence);
            self.previous = Some(snapshot);
            return Ok(Vec::new());
        };
        let mut admitted = Vec::new();
        let mut consumed = 0;
        while let Some(newline) = self.pending[consumed..].iter().position(|b| *b == b'\n') {
            let end = consumed + newline + 1;
            let raw = &self.pending[consumed..end];
            let text = String::from_utf8_lossy(raw);
            if let Some(header) = self.header.captures(&text) {
                let epoch: f64 = header[1]
                    .parse()
                    .map_err(|_| "Invalid Android log timestamp")?;
                let pid: u32 = header[2].parse().map_err(|_| "Invalid Android log PID")?;
                if epoch > snapshot.fence {
                    break;
                }
                self.continuation = false;
                if epoch >= self.earliest.unwrap_or(f64::INFINITY) {
                    if self.strategy == LogStrategy::AndroidUid {
                        self.continuation = true;
                    } else if let Some(identity) = snapshot.processes.get(&pid) {
                        if previous
                            .processes
                            .get(&pid)
                            .is_some_and(|p| p.start_ticks != identity.start_ticks)
                            && epoch < identity.started_before_epoch
                        {
                            return Err("Android PID reused across queued logs. Logs stopped with an identity gap".into());
                        }
                        self.continuation = epoch >= identity.started_before_epoch;
                    } else if let Some(identity) = previous.processes.get(&pid) {
                        if epoch <= previous.fence && epoch >= identity.started_before_epoch {
                            self.continuation = true;
                        } else {
                            return Err("Android PID exited before queued logs were verified. Logs stopped with an identity gap".into());
                        }
                    }
                }
                if self.continuation {
                    admitted.extend_from_slice(raw);
                }
            } else if text.starts_with("---------")
                || text.starts_with("logcat:")
                || self.malformed.is_match(&text)
            {
                self.continuation = false;
            } else if self.continuation {
                admitted.extend_from_slice(raw);
            }
            consumed = end;
        }
        self.pending.drain(..consumed);
        self.previous = Some(snapshot);
        Ok(admitted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn resolved(strategy: LogStrategy) -> ResolvedLogs {
        ResolvedLogs {
            attribution: LogAttribution {
                strategy,
                ..LogAttribution::default()
            },
        }
    }
    fn snapshot(fence: f64, values: &[(u32, u64, f64)]) -> IdentitySnapshot {
        IdentitySnapshot {
            fence,
            processes: values
                .iter()
                .map(|(pid, ticks, birth)| {
                    (
                        *pid,
                        ProcessIdentity {
                            start_ticks: *ticks,
                            started_before_epoch: *birth,
                        },
                    )
                })
                .collect(),
        }
    }
    #[test]
    fn fallback_admits_only_owned_lifetimes_and_their_multiline_bytes() {
        let mut filter = LogFilter::new(&resolved(LogStrategy::AndroidProcessMap));
        filter
            .verify(snapshot(1792022400.0, &[(123, 5, 1792022390.0)]))
            .unwrap();
        filter.append(b"orphan\n1792022400.1 123 456 I App: owned\nstack line\n1792022400.2 999 999 I Other: private\nprivate continuation\n1792022400.3 123 456 V App: verbose\n1792022400.4 malformed\nsecret orphan\n").unwrap();
        let admitted = filter
            .verify(snapshot(1792022401.0, &[(123, 5, 1792022390.0)]))
            .unwrap();
        assert_eq!(
            String::from_utf8(admitted).unwrap(),
            "1792022400.1 123 456 I App: owned\nstack line\n1792022400.3 123 456 V App: verbose\n"
        );
    }
    #[test]
    fn uid_filter_waits_for_fence_and_discards_history() {
        let mut filter = LogFilter::new(&resolved(LogStrategy::AndroidUid));
        filter.verify(snapshot(1792022400.0, &[])).unwrap();
        filter
            .append(b"1792022399.0 123 123 I App: history\n1792022402.0 123 123 D App: future\n")
            .unwrap();
        assert!(filter
            .verify(snapshot(1792022401.0, &[]))
            .unwrap()
            .is_empty());
        assert!(filter.has_pending());
        assert_eq!(
            filter.verify(snapshot(1792022403.0, &[])).unwrap(),
            b"1792022402.0 123 123 D App: future\n"
        );
    }
    #[test]
    fn identity_loss_clock_rollback_and_backlog_fail_closed() {
        let mut filter = LogFilter::new(&resolved(LogStrategy::AndroidProcessMap));
        filter
            .verify(snapshot(1792022400.0, &[(123, 5, 1792022390.0)]))
            .unwrap();
        filter
            .append(b"1792022400.5 123 123 I App: queued\n")
            .unwrap();
        assert!(filter
            .verify(snapshot(1792022401.0, &[]))
            .unwrap_err()
            .contains("identity gap"));
        assert!(filter
            .verify(snapshot(1792022399.0, &[]))
            .unwrap_err()
            .contains("rolled back"));
        assert!(filter.append(&vec![0; 2 * 1024 * 1024 + 1]).is_err());
    }
    #[test]
    fn pid_reuse_never_reattributes_queued_records_to_the_new_lifetime() {
        let mut filter = LogFilter::new(&resolved(LogStrategy::AndroidProcessMap));
        filter
            .verify(snapshot(1792022400.0, &[(123, 5, 1792022390.0)]))
            .unwrap();
        filter
            .append(b"1792022400.5 123 123 I App: old queued lifetime\n")
            .unwrap();
        assert!(filter
            .verify(snapshot(1792022402.0, &[(123, 10, 1792022401.0)]))
            .unwrap_err()
            .contains("identity gap"));
        let mut filter = LogFilter::new(&resolved(LogStrategy::AndroidProcessMap));
        filter
            .verify(snapshot(1792022400.0, &[(123, 5, 1792022390.0)]))
            .unwrap();
        filter
            .append(b"1792022401.5 123 123 D App: new lifetime\n")
            .unwrap();
        assert_eq!(
            filter
                .verify(snapshot(1792022402.0, &[(123, 10, 1792022401.0)]))
                .unwrap(),
            b"1792022401.5 123 123 D App: new lifetime\n"
        );
    }
    #[test]
    fn packages_are_user_scoped_and_shared_uids_remain_visible() {
        assert_eq!(
            packages("package:a.app uid:1010001\npackage:b.app uid:1010001", 10)
                .unwrap()
                .len(),
            2
        );
        assert!(packages("package:a.app uid:10001", 10).is_err());
        assert!(packages("package:a.app uid:10001\npackage:a.app uid:10002", 0).is_err());
        assert!(!valid_application_id("x'; reboot"));
        assert!(
            install_identity("codePath=/data/app/a\nfirstInstallTime=x\nlastUpdateTime=y").is_ok()
        );
        assert!(install_identity("codePath=/data/app/a\nfirstInstallTime=x").is_err());
    }
}
