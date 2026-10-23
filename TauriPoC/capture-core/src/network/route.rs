use crate::{lease, model::Result, process};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

const KEYS: [&str; 5] = [
    "http_proxy",
    "global_http_proxy_host",
    "global_http_proxy_port",
    "global_http_proxy_exclusion_list",
    "global_proxy_pac_url",
];
type Settings = BTreeMap<String, Option<String>>;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) enum Mutation {
    ReverseAdd,
    Setting(String, Option<String>),
    ClearEmpty,
    ClearDisabled,
    ReverseRemove,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Operation {
    mutation: Mutation,
    allowed_after: Settings,
    verified: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Route {
    pub adb: PathBuf,
    pub device_id: String,
    pub endpoint: String,
    before: Settings,
    applied: Settings,
    operations: Vec<Operation>,
    clear_acknowledged: bool,
    pub restored: bool,
    fixture: bool,
}
impl Route {
    pub fn prepare(adb: PathBuf, device_id: String, port: u16, fixture: bool) -> Result<Self> {
        let mut route = Self {
            adb,
            device_id,
            endpoint: format!("tcp:{port}"),
            before: Settings::new(),
            applied: Settings::new(),
            operations: vec![],
            clear_acknowledged: false,
            restored: false,
            fixture,
        };
        route.before = route.snapshot()?;
        route.applied = KEYS
            .into_iter()
            .zip([
                format!("127.0.0.1:{port}"),
                "127.0.0.1".into(),
                port.to_string(),
                "".into(),
                "".into(),
            ])
            .map(|(k, v)| (k.into(), Some(v)))
            .collect();
        if route.reverse()?.contains_key(&route.endpoint) {
            return Err("Android reverse endpoint is already occupied".into());
        }
        Ok(route)
    }
    pub fn save(&self, directory: &Path) -> Result<()> {
        self.probe(directory, "before_journal")?;
        lease::write(&directory.join("runtime/routing.json"), self)?;
        self.probe(directory, "after_journal")
    }
    fn intent(
        &mut self,
        directory: &Path,
        mutation: Mutation,
        allowed_after: Settings,
    ) -> Result<()> {
        self.operations.push(Operation {
            mutation,
            allowed_after,
            verified: false,
        });
        self.save(directory)
    }
    fn acknowledge(&mut self, directory: &Path) -> Result<()> {
        self.operations
            .last_mut()
            .ok_or("Missing route intent")?
            .verified = true;
        self.save(directory)
    }
    pub fn claim(&mut self, directory: &Path) -> Result<()> {
        self.intent(directory, Mutation::ReverseAdd, self.before.clone())?;
        self.mutate(
            directory,
            &[
                "reverse".into(),
                "--no-rebind".into(),
                self.endpoint.clone(),
                self.endpoint.clone(),
            ],
        )?;
        if self.observe_reverse(directory)?.get(&self.endpoint) != Some(&self.endpoint) {
            return Err("Reverse claim could not be verified".into());
        }
        self.acknowledge(directory)?;
        for key in KEYS.into_iter().rev() {
            self.set(directory, key, self.applied[key].clone())?;
        }
        if self.observe(directory)? != self.applied {
            return Err("Complete Android proxy setup could not be verified".into());
        }
        Ok(())
    }
    fn owned(&self, observed: &Settings) -> Result<()> {
        let unknown: Vec<_> = KEYS
            .into_iter()
            .filter(|key| {
                let current = &observed[*key];
                current != &self.before[*key]
                    && !self
                        .operations
                        .iter()
                        .any(|op| op.allowed_after.get(*key) == Some(current))
            })
            .collect();
        if !unknown.is_empty() {
            return Err(format!(
                "Android proxy changed outside capture. Preserved fields {}",
                unknown.join(", ")
            ));
        }
        Ok(())
    }
    fn set(&mut self, directory: &Path, key: &str, value: Option<String>) -> Result<()> {
        let mut observed = self.observe(directory)?;
        self.owned(&observed)?;
        if observed[key] == value {
            return Ok(());
        }
        observed.insert(key.into(), value.clone());
        self.intent(
            directory,
            Mutation::Setting(key.into(), value.clone()),
            observed.clone(),
        )?;
        self.put(directory, key, value.as_deref())?;
        if self.observe(directory)? != observed {
            return Err(format!("Android did not acknowledge restoring {key}"));
        }
        self.acknowledge(directory)
    }
    fn put(&self, directory: &Path, key: &str, value: Option<&str>) -> Result<()> {
        let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
        let script = match value {
            Some(v) => format!("settings put global {} {}", quote(key), quote(v)),
            None => format!("settings delete global {}", quote(key)),
        };
        self.mutate(directory, &["shell".into(), script])?;
        Ok(())
    }
    fn disabled(&self) -> bool {
        let get = |key: &str| self.before[key].as_deref();
        matches!(get(KEYS[0]), None | Some("" | ":0"))
            && matches!(get(KEYS[1]), None | Some(""))
            && matches!(get(KEYS[2]), None | Some("" | "0"))
            && matches!(get(KEYS[4]), None | Some(""))
    }
    fn cleared() -> Settings {
        KEYS.into_iter()
            .zip([":0", "", "0", "", ""])
            .map(|(k, v)| (k.into(), Some(v.into())))
            .collect()
    }
    pub fn restore(&mut self, directory: &Path) -> Result<()> {
        if self.restored {
            return Ok(());
        }
        let proxy_attempted = self.operations.iter().any(|o| {
            matches!(
                o.mutation,
                Mutation::Setting(..) | Mutation::ClearEmpty | Mutation::ClearDisabled
            )
        });
        if proxy_attempted {
            let current = self.observe(directory)?;
            self.owned(&current)?;
            if self.disabled() && (!self.clear_acknowledged || current != Self::cleared()) {
                self.clear_acknowledged = false;
                self.save(directory)?;
                if current[KEYS[0]].as_deref() == Some(":0") {
                    let mut empty = current;
                    empty.insert(KEYS[0].into(), Some("".into()));
                    self.intent(directory, Mutation::ClearEmpty, empty.clone())?;
                    self.put(directory, KEYS[0], Some(""))?;
                    if self.observe(directory)? != empty {
                        return Err(
                            "Android did not acknowledge the empty cached-proxy transition".into(),
                        );
                    }
                    self.acknowledge(directory)?;
                }
                self.owned(&self.observe(directory)?)?;
                self.intent(directory, Mutation::ClearDisabled, Self::cleared())?;
                self.put(directory, KEYS[0], Some(":0"))?;
                let deadline = Instant::now() + Duration::from_secs(2);
                loop {
                    let current = self.observe(directory)?;
                    self.owned(&current)?;
                    if current == Self::cleared() {
                        break;
                    }
                    if current[KEYS[0]].as_deref() != Some(":0") || Instant::now() >= deadline {
                        return Err("Android did not acknowledge clearing its cached proxy. Reconnect and retry".into());
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                self.acknowledge(directory)?;
                self.clear_acknowledged = true;
                self.save(directory)?;
            }
            for key in KEYS.into_iter().rev() {
                self.set(directory, key, self.before[key].clone())?;
            }
            if self.observe(directory)? != self.before {
                return Err("Exact original Android settings were not restored".into());
            }
        }
        if self
            .operations
            .iter()
            .any(|o| matches!(o.mutation, Mutation::ReverseAdd))
        {
            match self.observe_reverse(directory)?.get(&self.endpoint) {
                Some(target) if target != &self.endpoint => {
                    return Err("External ADB reverse mapping was preserved".into())
                }
                Some(_) => {
                    self.intent(directory, Mutation::ReverseRemove, self.before.clone())?;
                    self.mutate(
                        directory,
                        &["reverse".into(), "--remove".into(), self.endpoint.clone()],
                    )?;
                    if self
                        .observe_reverse(directory)?
                        .contains_key(&self.endpoint)
                    {
                        return Err("Reverse removal was not acknowledged".into());
                    }
                    self.acknowledge(directory)?;
                }
                None => {}
            }
        }
        if proxy_attempted && self.observe(directory)? != self.before {
            return Err("Android changed before restoration commit".into());
        }
        self.restored = true;
        self.save(directory)
    }
    fn probe(&self, directory: &Path, boundary: &str) -> Result<()> {
        if !self.fixture {
            return Ok(());
        }
        use std::io::Write;
        let path = directory.join("runtime/route-boundaries.jsonl");
        let mut trace = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        let count = std::fs::read(&path)
            .map_err(|e| e.to_string())?
            .iter()
            .filter(|b| **b == b'\n')
            .count()
            + 1;
        writeln!(
            trace,
            "{}",
            serde_json::json!({"index":count,"boundary":boundary})
        )
        .and_then(|_| trace.sync_data())
        .map_err(|e| e.to_string())?;
        let plan: Option<serde_json::Value> =
            std::fs::read(directory.join("runtime/fixture-crash.json"))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok());
        if plan.is_some_and(|p| p["at"].as_u64() == Some(count as u64)) {
            std::process::exit(91);
        }
        Ok(())
    }
    fn mutate(&self, directory: &Path, args: &[String]) -> Result<String> {
        self.probe(directory, "before_command")?;
        let value = self.command(args)?;
        self.probe(directory, "after_command")?;
        Ok(value)
    }
    fn observe(&self, directory: &Path) -> Result<Settings> {
        self.probe(directory, "before_verify")?;
        let value = self.snapshot()?;
        self.probe(directory, "after_verify")?;
        Ok(value)
    }
    fn observe_reverse(&self, directory: &Path) -> Result<BTreeMap<String, String>> {
        self.probe(directory, "before_verify")?;
        let value = self.reverse()?;
        self.probe(directory, "after_verify")?;
        Ok(value)
    }
    fn snapshot(&self) -> Result<Settings> {
        let output = self.command(&["shell".into(), "settings list global".into()])?;
        let all: BTreeMap<_, _> = output
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(k, v)| (k, v.to_owned()))
            .collect();
        Ok(KEYS
            .into_iter()
            .map(|key| (key.into(), all.get(key).cloned()))
            .collect())
    }
    fn reverse(&self) -> Result<BTreeMap<String, String>> {
        Ok(self
            .command(&["reverse".into(), "--list".into()])?
            .lines()
            .filter_map(|line| {
                let fields: Vec<_> = line.split_whitespace().collect();
                (fields.len() == 3).then(|| (fields[1].into(), fields[2].into()))
            })
            .collect())
    }
    fn command(&self, args: &[String]) -> Result<String> {
        let mut command = Command::new(&self.adb);
        process::quiet(&mut command);
        command
            .args(["-H", "127.0.0.1", "-P", "5037", "-s", &self.device_id])
            .args(args);
        let output =
            process::output_bounded(&mut command, Duration::from_secs(5), None, 65536, 8192)
                .map_err(|error| format!("ADB command failed: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "ADB connection unavailable. {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        String::from_utf8(output.stdout).map_err(|_| "ADB state was not UTF-8".into())
    }
}
