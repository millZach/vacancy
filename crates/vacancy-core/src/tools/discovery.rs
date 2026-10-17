//! Read-only capability evidence. Unknown schemas and incomplete output produce an explanation, not targets.

use serde::Deserialize;

use super::{DockerRead, LocalEndpoint, Output, Query, Runner, Status};
use crate::CancelToken;

/// A tool's availability is different from an available service or a supported output schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability {
    Ready,
    /// Tool answered, but its candidate/measurement schemas have not been authenticated for execution.
    Detected,
    Missing,
    Unavailable(String),
}

/// Raw bounded transcripts remain attached to the result for a run log or diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evidence {
    pub query: Query,
    pub output: Output,
}

/// The identity of the daemon that actually answered. No host-volume byte claim is made from Docker's figures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Docker {
    pub availability: Availability,
    pub daemon_id: Option<String>,
    pub context_name: Option<String>,
    pub endpoint: Option<String>,
    pub evidence: Vec<Evidence>,
}

/// Capture context before and after service queries. Every daemon query pins the verified local endpoint; the
/// native adapter removes environment endpoint/context overrides. Raw evidence remains non-executable.
pub fn docker(runner: &mut dyn Runner, cancel: &CancelToken) -> Docker {
    let mut out = Docker {
        availability: Availability::Ready,
        daemon_id: None,
        context_name: None,
        endpoint: None,
        evidence: Vec::new(),
    };
    let context = record(runner, &mut out.evidence, Query::DockerContext, cancel);
    if let Err(reason) = usable(&context) {
        out.availability = reason;
        return out;
    }
    #[derive(Deserialize)]
    struct Endpoint {
        #[serde(rename = "Host")]
        host: String,
    }
    #[derive(Deserialize)]
    struct Endpoints {
        docker: Endpoint,
    }
    #[derive(Deserialize)]
    struct Context {
        #[serde(rename = "Name")]
        name: String,
        #[serde(rename = "Endpoints")]
        endpoints: Endpoints,
    }
    let contexts: Vec<Context> = match serde_json::from_str(&context.stdout) {
        Ok(c) => c,
        Err(_) => {
            out.availability = unavailable("Docker context output was not recognized");
            return out;
        }
    };
    if contexts.len() != 1 || LocalEndpoint::parse(&contexts[0].endpoints.docker.host).is_none() {
        out.availability = unavailable("Docker is not using one verified local endpoint");
        return out;
    }
    out.context_name = Some(contexts[0].name.clone());
    out.endpoint = Some(contexts[0].endpoints.docker.host.clone());
    let endpoint =
        LocalEndpoint::parse(&contexts[0].endpoints.docker.host).expect("checked endpoint");
    let query = |read| Query::Docker {
        endpoint: endpoint.clone(),
        read,
    };
    let info = record(runner, &mut out.evidence, query(DockerRead::Info), cancel);
    if let Err(reason) = usable(&info) {
        out.availability = reason;
        return out;
    }
    #[derive(Deserialize)]
    struct Info {
        #[serde(rename = "ID")]
        id: String,
    }
    match serde_json::from_str::<Info>(&info.stdout) {
        Ok(info) if !info.id.is_empty() && info.id.len() <= 256 => out.daemon_id = Some(info.id),
        _ => {
            out.availability = unavailable("Docker daemon identity was not recognized");
            return out;
        }
    }
    for query in [
        DockerRead::Images,
        DockerRead::Containers,
        DockerRead::DiskUsage,
    ]
    .map(query)
    {
        let output = record(runner, &mut out.evidence, query, cancel);
        if let Err(reason) = usable(&output) {
            out.availability = reason;
            return out;
        }
    }
    let after = record(runner, &mut out.evidence, Query::DockerContext, cancel);
    if !after.complete() || after.stdout != context.stdout {
        out.availability = unavailable("Docker context changed during discovery");
    }
    if out.availability == Availability::Ready {
        out.availability = Availability::Detected;
    }
    out
}

/// One unavailable simulator device that can be reviewed by exact UUID. Runtimes deliberately remain advice:
/// a runtime's age does not establish that no recent project uses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimulatorDevice {
    pub uuid: String,
    pub name: String,
    pub runtime: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Simulators {
    pub availability: Availability,
    pub unavailable_devices: Vec<SimulatorDevice>,
    pub runtime_advice: String,
    pub evidence: Vec<Evidence>,
}

pub fn simulators(runner: &mut dyn Runner, cancel: &CancelToken) -> Simulators {
    let mut out = Simulators {
        availability: Availability::Ready,
        unavailable_devices: Vec::new(),
        runtime_advice: "Review simulator runtimes in Xcode Settings. Vacancy cannot establish that no recent project uses a runtime, so it will not remove one.".into(),
        evidence: Vec::new(),
    };
    let inventory = record(runner, &mut out.evidence, Query::SimulatorInventory, cancel);
    if let Err(reason) = usable(&inventory) {
        out.availability = reason;
        return out;
    }
    #[derive(Deserialize)]
    struct Device {
        udid: String,
        name: String,
        #[serde(rename = "isAvailable")]
        available: bool,
    }
    #[derive(Deserialize)]
    struct Inventory {
        devices: std::collections::BTreeMap<String, Vec<Device>>,
    }
    let data: Inventory = match serde_json::from_str(&inventory.stdout) {
        Ok(data) => data,
        Err(_) => {
            out.availability = unavailable("Simulator inventory was not recognized");
            return out;
        }
    };
    let mut ids = std::collections::HashSet::new();
    for (runtime, devices) in data.devices {
        for device in devices {
            if !uuid(&device.udid) || !ids.insert(device.udid.to_ascii_uppercase()) {
                out.availability =
                    unavailable("Simulator inventory has an invalid or duplicate device identity");
                out.unavailable_devices.clear();
                return out;
            }
            if !device.available {
                out.unavailable_devices.push(SimulatorDevice {
                    uuid: device.udid,
                    name: device.name,
                    runtime: runtime.clone(),
                });
            }
        }
    }
    out
}

fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

/// `brew cleanup --dry-run` is a preview, not a frozen plan or a byte measurement.
pub fn brew(runner: &mut dyn Runner, cancel: &CancelToken) -> Evidence {
    Evidence {
        query: Query::BrewPreview,
        output: runner.query(Query::BrewPreview, cancel),
    }
}

fn record(
    runner: &mut dyn Runner,
    evidence: &mut Vec<Evidence>,
    query: Query,
    cancel: &CancelToken,
) -> Output {
    let output = runner.query(query.clone(), cancel);
    evidence.push(Evidence {
        query,
        output: output.clone(),
    });
    output
}

fn unavailable(reason: &str) -> Availability {
    Availability::Unavailable(reason.into())
}

fn usable(output: &Output) -> Result<(), Availability> {
    if output.complete() {
        return Ok(());
    }
    Err(match &output.status {
        Status::Missing => Availability::Missing,
        Status::Cancelled => unavailable("Discovery cancelled"),
        Status::TimedOut => unavailable("The tool did not answer before its deadline"),
        Status::Refused(reason) => Availability::Unavailable(reason.clone()),
        Status::Success => {
            unavailable("The tool output exceeded its limit; no targets were inferred")
        }
        Status::Failed(_) => {
            unavailable("The tool or its service is unavailable; no cleanup was attempted")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::FakeRunner;

    fn output(text: &str) -> Output {
        Output {
            status: Status::Success,
            stdout: text.into(),
            stderr: String::new(),
            truncated: false,
        }
    }

    #[test]
    fn remote_daemon_is_never_queried() {
        let mut runner = FakeRunner::new([(
            Query::DockerContext,
            output(r#"[{"Name":"production","Endpoints":{"docker":{"Host":"ssh://server"}}}]"#),
        )]);
        let found = docker(&mut runner, &CancelToken::new());
        assert!(matches!(found.availability, Availability::Unavailable(_)));
        assert_eq!(runner.calls, [Query::DockerContext]);
    }

    #[test]
    fn exact_unavailable_devices_only_and_runtime_use_is_unknown() {
        let inventory = r#"{"devices":{"runtime-1":[{"udid":"12345678-1234-1234-1234-123456789012","name":"Old iPhone","isAvailable":false},{"udid":"AAAAAAAA-1234-1234-1234-123456789012","name":"iPhone","isAvailable":true}]}}"#;
        let mut runner = FakeRunner::new([(Query::SimulatorInventory, output(inventory))]);
        let found = simulators(&mut runner, &CancelToken::new());
        assert_eq!(found.availability, Availability::Ready);
        assert_eq!(found.unavailable_devices.len(), 1);
        assert_eq!(
            found.unavailable_devices[0].uuid,
            "12345678-1234-1234-1234-123456789012"
        );
        assert!(found.runtime_advice.contains("will not remove"));
    }

    #[test]
    fn bad_ids_truncation_and_missing_fake_never_infer_targets() {
        for result in [
            output(
                r#"{"devices":{"runtime":[{"udid":"--all; rm","name":"x","isAvailable":false}]}}"#,
            ),
            Output {
                truncated: true,
                ..output("{}")
            },
        ] {
            let mut runner = FakeRunner::new([(Query::SimulatorInventory, result)]);
            assert!(
                simulators(&mut runner, &CancelToken::new())
                    .unavailable_devices
                    .is_empty()
            );
        }
        let mut runner = FakeRunner::default();
        assert!(matches!(
            docker(&mut runner, &CancelToken::new()).availability,
            Availability::Unavailable(_)
        ));
    }
}
